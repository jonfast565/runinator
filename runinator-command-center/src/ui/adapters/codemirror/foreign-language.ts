import { StreamLanguage } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import type { TextEditorLanguage } from "../../../core/platform/text-editor";

const aliases: Record<string, TextEditorLanguage> = {
  py: "python",
  js: "javascript",
  node: "javascript",
  sh: "bash",
  "common-lisp": "commonlisp",
  common_lisp: "commonlisp",
  lisp: "commonlisp",
  cl: "commonlisp",
  sbcl: "commonlisp",
  cob: "cobol",
  gnucobol: "cobol",
  gcc: "c",
  c17: "c",
  "c++": "cpp",
  cxx: "cpp",
  cplusplus: "cpp",
  "g++": "cpp",
  f90: "fortran",
  f95: "fortran",
  gfortran: "fortran",
  adb: "ada",
  gnat: "ada",
  rb: "ruby",
  pl: "perl",
  golang: "go",
  pwsh: "powershell",
  ps1: "powershell",
  "c#": "csharp",
  cs: "csharp",
  "f#": "fsharp",
  fs: "fsharp",
  "vb.net": "vbnet",
  visualbasic: "vbnet",
  vb: "vbnet",
};

const supported = new Set<TextEditorLanguage>([
  "python",
  "javascript",
  "bash",
  "commonlisp",
  "cobol",
  "c",
  "cpp",
  "fortran",
  "ada",
  "ruby",
  "perl",
  "php",
  "go",
  "swift",
  "powershell",
  "csharp",
  "fsharp",
  "vbnet",
]);

export function canonicalForeignLanguage(language: string): TextEditorLanguage | null {
  const normalized = language.trim().toLowerCase();
  const canonical = aliases[normalized] ?? normalized;
  return supported.has(canonical) ? canonical : null;
}

export async function loadForeignLanguageExtension(
  language: TextEditorLanguage,
): Promise<Extension> {
  switch (language) {
    case "python": {
      const { python } = await import("@codemirror/legacy-modes/mode/python");
      return StreamLanguage.define(python);
    }

    case "javascript": {
      const { javascript } = await import("@codemirror/legacy-modes/mode/javascript");
      return StreamLanguage.define(javascript);
    }

    case "bash": {
      const { shell } = await import("@codemirror/legacy-modes/mode/shell");
      return StreamLanguage.define(shell);
    }

    case "commonlisp": {
      const { commonLisp } = await import("@codemirror/legacy-modes/mode/commonlisp");
      return StreamLanguage.define(commonLisp);
    }

    case "cobol": {
      const { cobol } = await import("@codemirror/legacy-modes/mode/cobol");
      return StreamLanguage.define(cobol);
    }

    case "c": {
      const { c } = await import("@codemirror/legacy-modes/mode/clike");
      return StreamLanguage.define(c);
    }

    case "cpp": {
      const { cpp } = await import("@codemirror/legacy-modes/mode/clike");
      return StreamLanguage.define(cpp);
    }

    case "fortran": {
      const { fortran } = await import("@codemirror/legacy-modes/mode/fortran");
      return StreamLanguage.define(fortran);
    }

    case "ada": {
      const { ada } = await import("./ada-mode");
      return StreamLanguage.define(ada);
    }

    case "ruby": {
      const { ruby } = await import("@codemirror/legacy-modes/mode/ruby");
      return StreamLanguage.define(ruby);
    }

    case "perl": {
      const { perl } = await import("@codemirror/legacy-modes/mode/perl");
      return StreamLanguage.define(perl);
    }

    case "php": {
      const { php } = await import("@codemirror/lang-php");
      return php();
    }

    case "go": {
      const { go } = await import("@codemirror/legacy-modes/mode/go");
      return StreamLanguage.define(go);
    }

    case "swift": {
      const { swift } = await import("@codemirror/legacy-modes/mode/swift");
      return StreamLanguage.define(swift);
    }

    case "powershell": {
      const { powerShell } = await import("@codemirror/legacy-modes/mode/powershell");
      return StreamLanguage.define(powerShell);
    }

    case "csharp": {
      const { csharp } = await import("@codemirror/legacy-modes/mode/clike");
      return StreamLanguage.define(csharp);
    }

    case "fsharp": {
      const { fSharp } = await import("@codemirror/legacy-modes/mode/mllike");
      return StreamLanguage.define(fSharp);
    }

    case "vbnet": {
      const { vb } = await import("@codemirror/legacy-modes/mode/vb");
      return StreamLanguage.define(vb);
    }

    default:
      return [];
  }
}
