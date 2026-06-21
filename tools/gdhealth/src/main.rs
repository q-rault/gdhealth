fn main() {
    if let Err(error) = gdhealth::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
