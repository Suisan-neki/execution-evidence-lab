"""Runs only inside a disposable container. The host never imports target packages."""
import base64
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.parse
import urllib.request

ROOT = Path("/work/source")
RESULT = Path("/result")
REPORT = dict(preparation="not_observed", target="not_observed", packages=[], steps=[], detail="")


def write_report():
    tmp = RESULT / "report.tmp"
    tmp.write_text(json.dumps(REPORT, ensure_ascii=False, indent=2))
    tmp.replace(RESULT / "report.json")


def call(args, *, cwd=ROOT, env=None):
    start = time.monotonic()
    step = dict(args=args, phase="target" if REPORT["preparation"] == "succeeded" else "preparation")
    REPORT["steps"].append(step)
    write_report()
    run = subprocess.run(args, cwd=cwd, env=env, check=False)
    step.update(exit_code=run.returncode, elapsed_ms=round((time.monotonic() - start) * 1000))
    write_report()
    if run.returncode:
        raise RuntimeError(f"command exited {run.returncode}: {args[0]}")


def download(url, path, algorithm, expected):
    h = hashlib.new(algorithm)
    with urllib.request.urlopen(url, timeout=120) as response, path.open("wb") as output:
        while chunk := response.read(1024 * 1024):
            h.update(chunk)
            output.write(chunk)
    actual = h.hexdigest() if algorithm == "sha256" else base64.b64encode(h.digest()).decode()
    if actual != expected:
        raise RuntimeError(f"distribution hash mismatch: {path.name}")


def extract(archive, destination):
    # The pinned Node image has Python 3.11.2, before tarfile's data filter.
    # Accept only regular files and directories; never create links or device files.
    total = 0
    with tarfile.open(archive) as tar:
        for count, member in enumerate(tar):
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts or not name.parts or count >= 100000:
                raise RuntimeError("unsafe archive path or member limit")
            target = destination.joinpath(*name.parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                total += member.size
                if total > 1024 * 1024 * 1024:
                    raise RuntimeError("archive expansion limit")
                target.parent.mkdir(parents=True, exist_ok=True)
                with tar.extractfile(member) as source, target.open("xb") as output:
                    shutil.copyfileobj(source, output)
                target.chmod(member.mode & 0o777)
            else:
                raise RuntimeError("archive links and special files are not accepted")


def verify_files(base, files):
    for f in files:
        path = base / f["path"]
        if not path.is_file() or path.is_symlink() or hashlib.sha256(path.read_bytes()).hexdigest() != f["sha256"]:
            raise RuntimeError(f"pinned file mismatch: {f['path']}")


def pnpm_setup(recipe, env):
    setup = recipe["setup"]
    manifest = json.loads((ROOT / "package.json").read_text())
    if manifest.get("packageManager") != "pnpm@" + setup["version"]:
        raise RuntimeError("packageManager differs from pinned pnpm")
    archive = Path("/work/pnpm.tgz")
    download(setup["url"], archive, "sha512", setup["integrity"].removeprefix("sha512-"))
    manager = Path("/work/manager")
    manager.mkdir()
    extract(archive, manager)
    native_archive = Path("/work/pnpm-native.tgz")
    download(setup["executable_url"], native_archive, "sha512", setup["executable_integrity"].removeprefix("sha512-"))
    native = Path("/work/native")
    native.mkdir()
    extract(native_archive, native)
    pnpm = manager / "package/pnpm"
    shutil.copyfile(native / "package/pnpm", pnpm)
    pnpm.chmod(0o755)
    wrapper = Path("/usr/local/bin/pnpm")
    wrapper.write_text(f'#!/bin/sh\nexec "{pnpm}" "$@"\n')
    wrapper.chmod(0o755)
    call(["node", "--version"], env=env)
    call(["pnpm", "--version"], env=env)
    # No reconciliation or automatic manager-version changes. Lifecycle scripts are visible in the plan notice.
    call(["pnpm", "install", "--frozen-lockfile", "--verify-store-integrity"], env=env)
    verify_files(ROOT, recipe["locks"])
    # The pinned lock contains tarball integrity; package.json hashes describe installed manifests, not all installed bytes.
    integrity = {}
    current = None
    for line in (ROOT / setup["lockfile"]).read_text().splitlines():
        if re.match(r"^  \S.*:$", line):
            current = line.strip()[:-1].strip("'")
        match = re.search(r"resolution: \{integrity: ([^,} ]+)", line)
        if match and current:
            integrity[current] = match[1]
    inventory_script = r"""
const fs=require('fs'),path=require('path'),crypto=require('crypto');
const seen=new Set(),out=[];
function visit(dir){
 if(!fs.existsSync(dir))return;
 const real=fs.realpathSync(dir);if(seen.has(real))return;seen.add(real);
 for(const item of fs.readdirSync(dir,{withFileTypes:true})){
  if(item.name.startsWith('.'))continue;
  const p=path.join(dir,item.name);
  if(item.name.startsWith('@')){visit(p);continue;}
  const f=path.join(p,'package.json');
  if(fs.existsSync(f)){
   const bytes=fs.readFileSync(f),j=JSON.parse(bytes);
   if(j.name&&j.version)out.push({name:j.name,version:j.version,hash:crypto.createHash('sha256').update(bytes).digest('hex'),hash_scope:'installed_package_json'});
  }
  visit(path.join(p,'node_modules'));
 }
}
visit('node_modules');
fs.writeFileSync('/result/packages.json',JSON.stringify(out));
"""
    call(["node", "-e", inventory_script], env=env)
    packages = json.loads((RESULT / "packages.json").read_text())
    for p in packages:
        p["lock_integrity"] = integrity.get(p["name"] + "@" + p["version"])
    packages.append(dict(name="pnpm", version=setup["version"], hash=setup["integrity"], hash_scope="distribution_sri", lock_integrity=setup["integrity"]))
    packages.append(dict(name="@pnpm/exe.linux-x64", version=setup["version"], hash=setup["executable_integrity"], hash_scope="distribution_sri", lock_integrity=setup["executable_integrity"]))
    REPORT["packages"] = sorted(packages, key=lambda p: (p["name"], p["version"], p["hash"]))
    (RESULT / "packages.json").write_text(json.dumps(REPORT["packages"], indent=2))
    return env


def python_setup(recipe, env):
    call([sys.executable, "-m", "venv", "/work/venv"], env=env)
    python = "/work/venv/bin/python"
    wheels_dir = Path("/work/wheels")
    wheels_dir.mkdir()
    requirements = []
    for wheel in recipe["setup"]["wheels"]:
        filename = urllib.parse.unquote(urllib.parse.urlparse(wheel["url"]).path.rsplit("/", 1)[-1])
        if "/" in filename or "\\" in filename:
            raise RuntimeError("invalid wheel filename")
        dest = wheels_dir / filename
        download(wheel["url"], dest, "sha256", wheel["sha256"])
        requirements.append(f"{dest} --hash=sha256:{wheel['sha256']}")
    lock = Path("/work/requirements.txt")
    lock.write_text("\n".join(requirements) + "\n")
    call([python, "-m", "pip", "install", "--no-index", "--no-deps", "--require-hashes", "-r", str(lock)], env=env)
    call([python, "-m", "pip", "check"], env=env)
    # Verify names/versions from installed distribution metadata, not filenames or requested requirements.
    installed = json.loads(subprocess.check_output([python, "-m", "pip", "list", "--format=json"], env=env))
    normalize = lambda s: re.sub(r"[-_.]+", "-", s).lower()
    actual = {normalize(p["name"]): p["version"] for p in installed}
    for w in recipe["setup"]["wheels"]:
        if actual.get(normalize(w["name"])) != w["version"]:
            raise RuntimeError(f"installed distribution differs: {w['name']}")
        REPORT["packages"].append(dict(name=w["name"], version=w["version"], hash=w["sha256"], hash_scope="wheel_sha256", lock_integrity=None))
    (RESULT / "packages.json").write_text(json.dumps(REPORT["packages"], indent=2))
    (RESULT / "pip-list.json").write_text(json.dumps(installed, indent=2))
    env["PATH"] = "/work/venv/bin:" + env["PATH"]
    env["PYTHONPATH"] = str(ROOT)
    return env


def main():
    RESULT.mkdir(exist_ok=True)
    ROOT.mkdir(parents=True, exist_ok=True)
    write_report()
    recipe = json.loads(Path("/eel/recipe.json").read_text())
    try:
        if platform.machine() != "x86_64":
            raise RuntimeError("expected Linux amd64")
        extract("/eel/source.tar", ROOT)
        verify_files(ROOT, recipe["locks"])
        verify_files(Path("/eel/support"), recipe["support"])
        env = {"PATH": os.environ["PATH"], "HOME": "/work/home", "LANG": "C.UTF-8", "CI": "true"}
        Path(env["HOME"]).mkdir()
        env.update(recipe["env"])
        if recipe["setup"]["kind"] == "pnpm":
            env = pnpm_setup(recipe, env)
        else:
            env = python_setup(recipe, env)
        if recipe["command_phase"] == "target":
            REPORT["preparation"] = "succeeded"
        write_report()
        call(recipe["command"], env=env)
        verify_files(ROOT, recipe["locks"])
        if recipe["command_phase"] == "preparation":
            REPORT["preparation"] = "succeeded"
        else:
            REPORT["target"] = "succeeded"
        REPORT["detail"] = "Dependency preparation and declared command completed; no application or clinical claim is inferred."
    except Exception as error:
        if REPORT["preparation"] == "succeeded":
            REPORT["target"] = "failed"
        else:
            REPORT["preparation"] = "failed"
        REPORT["detail"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        write_report()


if __name__ == "__main__":
    main()
