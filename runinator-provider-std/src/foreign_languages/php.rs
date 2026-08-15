use super::ForeignLanguageAdapter;

pub(super) static PHP: Php = Php;

pub(super) struct Php;

impl ForeignLanguageAdapter for Php {
    fn canonical(&self) -> &'static str {
        "php"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.php"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.php"
    }

    fn runner_source(&self) -> &'static str {
        r#"<?php
declare(strict_types=1);
require __DIR__ . "/foreign.php";

if (!function_exists("main")) {
    throw new RuntimeException("foreign code must define main");
}
$context = json_decode(file_get_contents(getenv("RUNINATOR_CONTEXT")), true, 512, JSON_THROW_ON_ERROR);
$result = main($context);
file_put_contents(getenv("RUNINATOR_OUTPUT"), json_encode($result, JSON_THROW_ON_ERROR));
"#
    }

    fn execute(&self) -> &'static str {
        "php /work/runinator_runner.php"
    }
}
