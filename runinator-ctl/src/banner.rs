// figlet banner printed before command output on interactive runs.
const BANNER: &str = r#"
 ____  _   _ _   _ ___ _   _    _  _____ ___  ____
|  _ \| | | | \ | |_ _| \ | |  / \|_   _/ _ \|  _ \
| |_) | | | |  \| || ||  \| | / _ \ | || | | | |_) |
|  _ <| |_| | |\  || || |\  |/ ___ \| || |_| |  _ <
|_| \_\\___/|_| \_|___|_| \_/_/   \_\_| \___/|_| \_\
"#;

/// print the runinator figlet banner to stderr so it never pollutes json/stdout output.
pub fn print() {
    eprintln!("{BANNER}");
}
