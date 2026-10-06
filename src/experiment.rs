use crate::{
    assessment::assess,
    model::*,
    protocol::{read_frame, write_frame},
    recording::*,
};
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::Path,
    thread,
    time::{Duration, Instant},
};
const IO_TIMEOUT: Duration = Duration::from_millis(1000);

fn accept_until(listener: &TcpListener) -> io::Result<Option<TcpStream>> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + IO_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_read_timeout(Some(IO_TIMEOUT))?;
                stream.set_write_timeout(Some(IO_TIMEOUT))?;
                return Ok(Some(stream));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error),
        }
    }
}

fn serve(
    listener: TcpListener,
    directory: &Path,
    expected: &Request,
    fault: Fault,
    recorder: Recorder,
) -> io::Result<()> {
    let Some(mut stream) = accept_until(&listener)? else {
        return Ok(());
    };
    let request: Request = read_frame(&mut stream)?;
    if request.correlation != expected.correlation || request.data != expected.data {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected request correlation or payload",
        ));
    }
    recorder.record(
        Source::Server,
        EventKind::RequestReceived,
        "受信した識別子と入力を照合した",
    )?;
    let status = if !is_valid(&request.data) {
        recorder.record(
            Source::Server,
            EventKind::InputRejected,
            "名前が空、または年齢が150を超える",
        )?;
        ReplyStatus::InvalidInput
    } else if fault == Fault::FailBeforeSave {
        recorder.record(
            Source::Server,
            EventKind::SaveFailed,
            "実験条件により書き込み開始前に失敗させた",
        )?;
        ReplyStatus::SaveFailed
    } else {
        match write_json_new(&directory.join("stored.json"), &request.data) {
            Ok(()) => {
                if fault != Fault::OmitSaveObservation {
                    recorder.record(
                        Source::Server,
                        EventKind::SaveCommitted,
                        "ファイル書き込みとsync_allが成功した",
                    )?;
                }
                ReplyStatus::Saved
            }
            Err(error) => {
                recorder.record(Source::Server, EventKind::SaveFailed, error.to_string())?;
                ReplyStatus::SaveFailed
            }
        }
    };
    write_frame(
        &mut stream,
        &Reply {
            correlation: request.correlation,
            status,
        },
    )
}

// 評価側が保存ファイルを読む。観測側のassessには渡さない。
pub fn verify_storage(directory: &Path, input: &Data) -> Truth {
    match File::open(directory.join("stored.json")) {
        Ok(file) => match serde_json::from_reader::<_, Data>(file) {
            Ok(data) => {
                let matches_input = &data == input;
                Truth::Saved {
                    data,
                    matches_input,
                }
            }
            Err(error) => Truth::Unreadable {
                reason: error.to_string(),
            },
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Truth::NotSaved,
        Err(error) => Truth::Unreadable {
            reason: error.to_string(),
        },
    }
}

pub fn make_manifest(correlation: Correlation, condition: Condition) -> io::Result<Manifest> {
    Ok(Manifest {
        schema_version: 1,
        correlation,
        previous_run: None,
        source_revision: env!("SOURCE_REVISION").into(),
        source_fingerprint: env!("SOURCE_FINGERPRINT").into(),
        program_version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        started_unix_ms: unix_ms()?,
        mode: "demo".into(),
        fault: None,
        input: None,
        condition,
        timeout_ms: IO_TIMEOUT.as_millis() as u64,
        initial_storage: "新規の試行ディレクトリ。stored.jsonは存在しない".into(),
        observation_scope:
            "クライアント・サーバー・遮断用ゲートへの計測。OSやパケットの観測は未実装".into(),
        command: vec![],
    })
}

pub fn finish_run(
    directory: std::path::PathBuf,
    manifest: Manifest,
    truth: Truth,
) -> io::Result<RunResult> {
    let assessment = assess(&manifest, &load_events(&directory)?);
    write_json_new(&directory.join("assessment.json"), &assessment)?;
    write_json_new(&directory.join("truth.json"), &truth)?;
    let mut report = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("report.md"))?;
    writeln!(
        report,
        "# 試行 {}\n\n条件：{}\n\n判定：{}\n\n理由：{}\n\n根拠：{}\n\n次の確認：{}\n\n## 未確認事項\n",
        manifest.correlation.run_id,
        manifest.condition.text,
        assessment.verdict.label(),
        assessment.reason,
        assessment.evidence.join(", "),
        assessment.next_action
    )?;
    for item in &assessment.unconfirmed {
        writeln!(report, "- {item}")?;
    }
    report.sync_all()?;
    write_json_new(
        &directory.join("complete.json"),
        &serde_json::json!({"schema_version": 1, "run_id": manifest.correlation.run_id}),
    )?;
    Ok(RunResult {
        directory,
        manifest,
        assessment,
        truth,
    })
}

pub fn run_trial(config: TrialConfig) -> io::Result<RunResult> {
    if !is_valid(&config.data) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "名前を入力し、年齢を0〜150にしてください",
        ));
    }
    let (directory, correlation) = new_run_directory(&config.output)?;
    let mut manifest = make_manifest(correlation.clone(), Condition::demo())?;
    manifest.input = Some(config.data.clone());
    manifest.fault = Some(config.fault);
    manifest.previous_run = config.previous_run;
    write_json_new(&directory.join("manifest.json"), &manifest)?;
    let recorder = Recorder::new(&directory, correlation.clone())?;
    let request = Request {
        correlation,
        data: config.data,
    };
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let server_address = listener.local_addr()?;
    let server_directory = directory.clone();
    let server_request = request.clone();
    let server_recorder = recorder.clone();
    let fault = config.fault;
    let server = thread::spawn(move || {
        serve(
            listener,
            &server_directory,
            &server_request,
            fault,
            server_recorder,
        )
    });
    // 本物のTCP送信をゲートが受け取り、サーバーへ転送せず切断する。
    let (address, gate) = if fault == Fault::DisconnectBeforeReceive {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let gate_recorder = recorder.clone();
        let expected = request.clone();
        let gate = thread::spawn(move || -> io::Result<()> {
            let mut stream = accept_until(&listener)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "gate accept timeout"))?;
            let blocked: Request = read_frame(&mut stream)?;
            if blocked.correlation != expected.correlation || blocked.data != expected.data {
                return Err(io::Error::other("unexpected request at gate"));
            }
            gate_recorder.record(
                Source::FaultGate,
                EventKind::RequestBlocked,
                "要求を読んだ後、サーバーへ転送せずTCP接続を閉じた",
            )?;
            stream.shutdown(Shutdown::Both)
        });
        (address, Some(gate))
    } else {
        (server_address, None)
    };
    recorder.record(
        Source::Client,
        EventKind::OperationStarted,
        "架空データ1件の送信操作",
    )?;
    let client = (|| -> io::Result<()> {
        let mut stream = TcpStream::connect_timeout(&address, IO_TIMEOUT)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        write_frame(&mut stream, &request)?;
        recorder.record(
            Source::Client,
            EventKind::RequestSent,
            "要求をTCPソケットへ書いた。受信・保存の完了は示さない",
        )?;
        let reply: Reply = read_frame(&mut stream)?;
        if reply.correlation != request.correlation {
            return Err(io::Error::other("reply correlation mismatch"));
        }
        recorder.record(
            Source::Client,
            EventKind::ReplyReceived,
            format!("{:?}", reply.status),
        )?;
        let (kind, detail) = if reply.status == ReplyStatus::Saved {
            (
                EventKind::DisplaySaved,
                "保存処理完了の応答を受信。後の閲覧やクラッシュ復旧は未確認",
            )
        } else {
            (EventKind::DisplayFailed, "保存成功の応答を受信していない")
        };
        recorder.record(Source::Client, kind, detail)
    })();
    if let Err(error) = client {
        recorder.record(
            Source::Client,
            EventKind::TransportFailed,
            error.to_string(),
        )?;
        recorder.record(
            Source::Client,
            EventKind::DisplayFailed,
            "通信が完了せず、保存成功を表示しない",
        )?;
    }
    if let Some(gate) = gate {
        gate.join()
            .map_err(|_| io::Error::other("gate thread panicked"))??;
    }
    server
        .join()
        .map_err(|_| io::Error::other("server thread panicked"))??;
    let truth = verify_storage(&directory, &request.data);
    finish_run(directory, manifest, truth)
}
