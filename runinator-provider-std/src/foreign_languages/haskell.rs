use super::ForeignLanguageAdapter;

pub(super) static HASKELL: Haskell = Haskell;

pub(super) struct Haskell;

impl ForeignLanguageAdapter for Haskell {
    fn canonical(&self) -> &'static str {
        "haskell"
    }

    fn source_filename(&self) -> &'static str {
        "Foreign.hs"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.sh"
    }

    fn runner_source(&self) -> &'static str {
        r#"#!/usr/bin/env bash
set -euo pipefail

mkdir -p /tmp/runinator-haskell
ghc -O2 -Wall \
    -outputdir /tmp/runinator-haskell \
    -i/work \
    /work/Main.hs \
    /work/Foreign.hs \
    -o /tmp/runinator_foreign
/tmp/runinator_foreign "$@"
"#
    }

    fn additional_files(&self) -> &'static [(&'static str, &'static str)] {
        &[("Main.hs", HASKELL_MAIN)]
    }

    fn execute(&self) -> &'static str {
        "bash /work/runinator_runner.sh"
    }
}

const HASKELL_MAIN: &str = r#"{-# LANGUAGE FlexibleInstances #-}

module Main where

import Data.Aeson (Value, eitherDecode, encode)
import qualified Data.ByteString.Lazy as ByteString
import Foreign (runinatorMain)
import System.Environment (getEnv)
import System.Exit (die)

class IntoRuninatorIO result where
    intoRuninatorIO :: result -> IO Value

instance IntoRuninatorIO Value where
    intoRuninatorIO = pure

instance IntoRuninatorIO (IO Value) where
    intoRuninatorIO = id

main :: IO ()
main = do
    contextPath <- getEnv "RUNINATOR_CONTEXT"
    outputPath <- getEnv "RUNINATOR_OUTPUT"
    payload <- ByteString.readFile contextPath
    context <- case eitherDecode payload of
        Left message -> die ("invalid Runinator context JSON: " ++ message)
        Right value -> pure value
    result <- intoRuninatorIO (runinatorMain context)
    ByteString.writeFile outputPath (encode result)
"#;
