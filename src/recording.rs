use crate::model::*;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
struct State {
    file: File,
    sequence: u64,
}
#[derive(Clone)]
pub struct Recorder {
    state: Arc<Mutex<State>>,
    correlation: Correlation,
    started: Instant,
}
impl Recorder {
    pub fn new(directory: &Path, correlation: Correlation) -> io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("events.jsonl"))?;
        Ok(Self {
            state: Arc::new(Mutex::new(State { file, sequence: 0 })),
            correlation,
            started: Instant::now(),
        })
    }
    pub fn record(
        &self,
        source: Source,
        kind: EventKind,
        detail: impl Into<String>,
    ) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("recorder lock poisoned"))?;
        state.sequence += 1;
        let event = Event {
            sequence: state.sequence,
            elapsed_us: self.started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
            correlation: self.correlation.clone(),
            source,
            kind,
            detail: detail.into(),
        };
        serde_json::to_writer(&mut state.file, &event).map_err(io::Error::other)?;
        state.file.write_all(b"\n")?;
        state.file.flush()
    }
}
pub fn unix_ms() -> io::Result<u64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis();
    u64::try_from(millis).map_err(io::Error::other)
}
pub fn new_run_directory(root: &Path) -> io::Result<(PathBuf, Correlation)> {
    fs::create_dir_all(root)?;
    let stamp = unix_ms()?;
    for counter in 0..10_000 {
        let run_id = format!("run-{stamp}-{}-{counter}", std::process::id());
        let directory = root.join(&run_id);
        match fs::create_dir(&directory) {
            Ok(()) => {
                return Ok((
                    directory,
                    Correlation {
                        record_id: format!("{run_id}:record-1"),
                        attempt_id: format!("{run_id}:attempt-1"),
                        run_id,
                    },
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(
        "could not allocate a unique run directory",
    ))
}
pub fn write_json_new(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, value).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()
}
pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_reader(BufReader::new(File::open(path)?)).map_err(io::Error::other)
}
pub fn load_events(directory: &Path) -> io::Result<Vec<Event>> {
    let events: Vec<Event> = BufReader::new(File::open(directory.join("events.jsonl"))?)
        .lines()
        .map(|line| serde_json::from_str(&line?).map_err(io::Error::other))
        .collect::<io::Result<_>>()?;
    let mut last_elapsed = 0;
    for (index, event) in events.iter().enumerate() {
        if event.sequence != index as u64 + 1 || event.elapsed_us < last_elapsed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "event sequence or elapsed time is invalid",
            ));
        }
        last_elapsed = event.elapsed_us;
    }
    Ok(events)
}
pub fn load_manifest(directory: &Path) -> io::Result<Manifest> {
    let manifest: Manifest = read_json(&directory.join("manifest.json"))?;
    if manifest.schema_version != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported manifest schema",
        ));
    }
    Ok(manifest)
}
