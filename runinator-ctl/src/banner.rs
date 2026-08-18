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

/// the banner without its surrounding blank lines, for a caller that is placing it itself.
///
/// the console is that caller: it prints the banner *after* the interface is up, so the capture
/// puts it at the top of the output pane. printed before, it would land on the screen the console
/// takes over a moment later and never be seen.
pub fn text() -> &'static str {
    BANNER.trim_matches('\n')
}
