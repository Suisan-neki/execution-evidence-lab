mod cli;

fn main() {
    if let Err(error) = cli::run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("エラー: {error}");
        std::process::exit(1);
    }
}
