"""Pinned synthetic CPU workflow with durable stage records and real faults."""
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import numpy as np
import torch
import monai
from monai.data import create_test_image_2d
from monai.inferers import sliding_window_inference
from monai.losses import DiceLoss
from monai.networks.nets import UNet
from monai.transforms import Compose, EnsureType, ScaleIntensity
from monai.utils import set_determinism

OUT = Path("/result")
FAULT = os.environ.get("EEL_MONAI_FAULT", "normal")
ATOL, RTOL = 1e-6, 1e-5
STAGES = ["fixture", "input_read", "preprocess", "train", "reference_inference", "checkpoint_save",
          "checkpoint_load", "inference", "result_save", "readback"]
CONFIG = dict(spatial_dims=2, in_channels=1, out_channels=1, channels=[4, 8], strides=[2],
              num_res_units=0, norm="BATCH", dropout=0.0)
record = dict(schema_version=1, source_commit=os.environ["EEL_MONAI_SOURCE_COMMIT"],
              monai_reported_version=monai.__version__, torch=torch.__version__, numpy=np.__version__,
              device="cpu", seed=144, model=CONFIG, optimizer="Adam", learning_rate=0.001,
              updates=3, atol=ATOL, rtol=RTOL, fault=FAULT, completed=False, outcome="not_observed",
              stages=[dict(id=name, status="not_observed", detail="") for name in STAGES],
              metrics={}, fault_actions=[], artifacts=[], limit="Synthetic process/record check; no clinical or model-performance evaluation.")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def persist():
    record["artifacts"] = [dict(path=str(p.relative_to(OUT)), bytes=p.stat().st_size, sha256=digest(p))
                           for p in sorted(OUT.rglob("*")) if p.is_file() and p.name not in
                           ("workflow.json", "workflow.tmp", "report.json", "report.tmp")]
    temporary = OUT / "workflow.tmp"
    with temporary.open("w") as output:
        json.dump(record, output, indent=2)
        output.flush()
        os.fsync(output.fileno())
    temporary.replace(OUT / "workflow.json")


@contextmanager
def stage(name):
    current = next(s for s in record["stages"] if s["id"] == name)
    current.update(status="running", started_monotonic_ns=time.monotonic_ns())
    persist()
    try:
        yield
    except Exception as error:
        current.update(status="failed", detail=f"{type(error).__name__}: {error}")
        persist()
        raise
    else:
        current.update(status="succeeded", ended_monotonic_ns=time.monotonic_ns())
        persist()


def save_array(path, array):
    with path.open("xb") as output:
        np.save(output, array, allow_pickle=False)
        output.flush()
        os.fsync(output.fileno())


def model_hash(model):
    h = hashlib.sha256()
    for name, tensor in sorted(model.state_dict().items()):
        array = tensor.detach().cpu().contiguous().numpy()
        h.update(name.encode())
        h.update(str(array.shape).encode())
        h.update(str(array.dtype).encode())
        h.update(array.tobytes())
    return h.hexdigest()


def main():
    if FAULT not in ["normal", "input_missing", "checkpoint_corrupt", "save_is_directory", "interrupted"]:
        raise ValueError("unknown declared fault")
    torch.set_num_threads(1)
    torch.use_deterministic_algorithms(True)
    set_determinism(144)
    with stage("fixture"):
        image, labels = create_test_image_2d(64, 64, num_objs=4, rad_min=3, rad_max=10, noise_max=0.1,
                                           random_state=np.random.RandomState(144))
        save_array(OUT / "input.npy", image.astype(np.float32))
        save_array(OUT / "labels.npy", (labels > 0).astype(np.float32))
        record["metrics"]["input_sha256"] = digest(OUT / "input.npy")
        record["metrics"]["labels_sha256"] = digest(OUT / "labels.npy")
    if FAULT == "input_missing":
        (OUT / "input.npy").unlink()
        record["fault_actions"].append(dict(kind=FAULT, path="input.npy", absent=not (OUT / "input.npy").exists()))
        persist()
    with stage("input_read"):
        image = np.load(OUT / "input.npy", allow_pickle=False)
        labels = np.load(OUT / "labels.npy", allow_pickle=False)
        if image.shape != (64, 64) or labels.shape != image.shape:
            raise ValueError("input shape differs")
        if digest(OUT / "input.npy") != record["metrics"]["input_sha256"]:
            raise ValueError("input file differs from generated fixture")
    with stage("preprocess"):
        transform = Compose([ScaleIntensity(minv=0.0, maxv=1.0), EnsureType(dtype=torch.float32, track_meta=False)])
        x = transform(image)[None, None]
        target = torch.from_numpy(labels)[None, None]
        if not torch.isfinite(x).all() or x.shape != (1, 1, 64, 64):
            raise ValueError("preprocessed tensor differs")
        record["metrics"]["preprocessed_sha256"] = hashlib.sha256(x.contiguous().numpy().tobytes()).hexdigest()
    with stage("train"):
        model = UNet(**CONFIG).cpu()
        record["metrics"]["initial_model_sha256"] = model_hash(model)
        optimizer = torch.optim.Adam(model.parameters(), lr=0.001)
        loss_fn = DiceLoss(sigmoid=True)
        model.train()
        losses = []
        for update in range(3):
            optimizer.zero_grad(set_to_none=True)
            loss = loss_fn(model(x), target)
            if not torch.isfinite(loss):
                raise ValueError("training loss is not finite")
            loss.backward()
            optimizer.step()
            losses.append(float(loss.detach()))
            record["metrics"].update(losses=losses, completed_updates=update + 1)
            persist()
            if FAULT == "interrupted" and update == 0:
                record["fault_actions"].append(dict(kind=FAULT, signal="SIGKILL", requested_after_updates=1))
                persist()
                os.kill(os.getpid(), signal.SIGKILL)
        record["metrics"]["trained_model_sha256"] = model_hash(model)
        if record["metrics"]["trained_model_sha256"] == record["metrics"]["initial_model_sha256"]:
            raise ValueError("training did not change model state")
    with stage("reference_inference"):
        model.eval()
        with torch.inference_mode():
            reference = model(x).detach().cpu()
        if reference.shape != x.shape or not torch.isfinite(reference).all():
            raise ValueError("reference inference differs")
        save_array(OUT / "reference.npy", reference.numpy())
    with stage("checkpoint_save"):
        checkpoint = OUT / "checkpoint.pt"
        if FAULT == "save_is_directory":
            checkpoint.mkdir()
            record["fault_actions"].append(dict(kind=FAULT, path="checkpoint.pt", is_directory=checkpoint.is_dir()))
            persist()
        with checkpoint.open("xb") as output:
            torch.save(dict(model=model.state_dict(), config=CONFIG, input_sha256=record["metrics"]["input_sha256"]), output)
            output.flush()
            os.fsync(output.fileno())
        record["metrics"]["saved_checkpoint_sha256"] = digest(checkpoint)
    if FAULT == "checkpoint_corrupt":
        checkpoint.write_bytes(b"deliberately invalid synthetic checkpoint")
        record["fault_actions"].append(dict(kind=FAULT, path="checkpoint.pt", sha256=digest(checkpoint)))
        persist()
    with stage("checkpoint_load"):
        loaded = torch.load(checkpoint, map_location="cpu", weights_only=True)
        if loaded["config"] != CONFIG or loaded["input_sha256"] != record["metrics"]["input_sha256"]:
            raise ValueError("checkpoint configuration or input differs")
        restored = UNet(**loaded["config"]).cpu()
        restored.load_state_dict(loaded["model"])
        restored.eval()
        record["metrics"]["restored_model_sha256"] = model_hash(restored)
        if record["metrics"]["restored_model_sha256"] != record["metrics"]["trained_model_sha256"]:
            raise ValueError("restored model state differs")
    with stage("inference"):
        with torch.inference_mode():
            prediction = sliding_window_inference(x, (64, 64), 1, restored, overlap=0.0, device="cpu", sw_device="cpu")
        torch.testing.assert_close(prediction, reference, atol=ATOL, rtol=RTOL)
        if prediction.shape != x.shape or not torch.isfinite(prediction).all():
            raise ValueError("restored inference shape or finiteness differs")
        record["metrics"]["max_abs_error"] = float((prediction - reference).abs().max())
    with stage("result_save"):
        save_array(OUT / "prediction.npy", prediction.detach().cpu().numpy())
        record["metrics"]["prediction_sha256"] = digest(OUT / "prediction.npy")
    with stage("readback"):
        subprocess.run([sys.executable, "/eel/support/readback.py"], check=True)
        readback = json.loads((OUT / "readback.json").read_text())
        if readback["file_sha256"] != record["metrics"]["prediction_sha256"]:
            raise ValueError("readback file differs")
        record["metrics"]["readback_max_abs_error"] = readback["max_abs_error"]


try:
    persist()
    main()
except Exception as error:
    record.update(completed=True, outcome="failed", error=f"{type(error).__name__}: {error}")
    persist()
    raise
else:
    record.update(completed=True, outcome="succeeded")
    persist()
    print(json.dumps(record))
