use super::ForeignLanguageAdapter;

pub(super) static ERLANG: Erlang = Erlang;

pub(super) struct Erlang;

impl ForeignLanguageAdapter for Erlang {
    fn canonical(&self) -> &'static str {
        "erlang"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.erl"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.escript"
    }

    fn runner_source(&self) -> &'static str {
        r#"#!/usr/bin/env escript
%%! -noshell
-mode(compile).

main(_Arguments) ->
    load_foreign(),
    {ok, ContextBytes} = file:read_file(os:getenv("RUNINATOR_CONTEXT")),
    Context = jiffy:decode(ContextBytes, [return_maps]),
    Result = foreign:runinator_main(Context),
    ok = file:write_file(os:getenv("RUNINATOR_OUTPUT"), jiffy:encode(Result)).

load_foreign() ->
    Options = [binary, return_errors, return_warnings],
    case compile:file("/work/foreign.erl", Options) of
        {ok, foreign, Beam} ->
            {module, foreign} = code:load_binary(foreign, "/work/foreign.erl", Beam),
            ok;
        {ok, foreign, Beam, _Warnings} ->
            {module, foreign} = code:load_binary(foreign, "/work/foreign.erl", Beam),
            ok;
        {ok, Module, _Beam} ->
            erlang:error({expected_foreign_module, Module});
        {ok, Module, _Beam, _Warnings} ->
            erlang:error({expected_foreign_module, Module});
        {error, Errors, Warnings} ->
            erlang:error({foreign_compile_failed, Errors, Warnings})
    end.
"#
    }

    fn execute(&self) -> &'static str {
        "escript /work/runinator_runner.escript"
    }
}
