import { isTauriRuntime } from "./ui/adapters/tauri/runtime";
import { createBrowserPlatformAdapter } from "./ui/adapters/browser/platform";
import { createTauriPlatformAdapter } from "./ui/adapters/tauri/platform";
import { setPlatformAdapter, setTextEditorHostFactory } from "./core/platform";
import { createCodeMirrorTextEditorHostFactory } from "./ui/adapters/codemirror/text-editor-host";

const platform = isTauriRuntime() ? createTauriPlatformAdapter() : createBrowserPlatformAdapter();

setPlatformAdapter(platform);
setTextEditorHostFactory(createCodeMirrorTextEditorHostFactory());
