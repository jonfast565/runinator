use super::ForeignLanguageAdapter;

pub(super) static PERL: Perl = Perl;

pub(super) struct Perl;

impl ForeignLanguageAdapter for Perl {
    fn canonical(&self) -> &'static str {
        "perl"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.pl"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.pl"
    }

    fn runner_source(&self) -> &'static str {
        r#"use strict;
use warnings;
use FindBin qw($Bin);
use JSON::PP qw(decode_json encode_json);

my $loaded = do "$Bin/foreign.pl";
die $@ if $@;
die $! unless defined $loaded;
die "foreign code must define main\n" unless defined &main;

open my $context_file, "<", $ENV{"RUNINATOR_CONTEXT"} or die $!;
local $/;
my $context = decode_json(<$context_file>);
close $context_file;
my $result = main($context);
open my $output_file, ">", $ENV{"RUNINATOR_OUTPUT"} or die $!;
print {$output_file} encode_json($result);
close $output_file;
"#
    }

    fn execute(&self) -> &'static str {
        "perl /work/runinator_runner.pl"
    }
}
