use super::ForeignLanguageAdapter;

pub(super) static COMMON_LISP: CommonLisp = CommonLisp;

pub(super) struct CommonLisp;

impl ForeignLanguageAdapter for CommonLisp {
    fn canonical(&self) -> &'static str {
        "commonlisp"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.lisp"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.lisp"
    }

    fn runner_source(&self) -> &'static str {
        r#"(require :asdf)
(asdf:load-system :yason)
(load (merge-pathnames "foreign.lisp" *load-truename*))

(unless (fboundp 'main)
  (error "foreign code must define main(context)"))

(let* ((context-path (uiop:getenv "RUNINATOR_CONTEXT"))
       (output-path (uiop:getenv "RUNINATOR_OUTPUT"))
       (context (with-open-file (stream context-path
                                        :direction :input
                                        :external-format :utf-8)
                  (yason:parse stream)))
       (result (funcall 'main context)))
  (with-open-file (stream output-path
                          :direction :output
                          :if-exists :supersede
                          :if-does-not-exist :create
                          :external-format :utf-8)
    (yason:encode result stream)))
"#
    }

    fn execute(&self) -> &'static str {
        "sbcl --noinform --disable-debugger --script /work/runinator_runner.lisp"
    }
}
