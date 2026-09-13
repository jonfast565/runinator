//! Mission creation is recipe-driven rather than restricted to a closed kind enum.

#[test]
fn mission_start_has_no_closed_kind_argument() {
    use clap::CommandFactory;

    let command = runinator_ctl_core::cli::Cli::command();
    let start = command
        .find_subcommand("missions")
        .and_then(|missions| missions.find_subcommand("start"))
        .expect("missions start command");
    assert!(
        start
            .get_arguments()
            .all(|argument| argument.get_id() != "kind")
    );
}
