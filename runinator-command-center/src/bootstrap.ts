import { isTauriRuntime } from "./ui/adapters/tauri/runtime";
import { createBrowserPlatformAdapter } from "./ui/adapters/browser/platform";
import { createTauriPlatformAdapter } from "./ui/adapters/tauri/platform";
import { setPlatformAdapter, setTextEditorHostFactory } from "./core/platform";
import { createCodeMirrorTextEditorHostFactory } from "./ui/adapters/codemirror/text-editor-host";
import { displayPreferencesService } from "./core/services";
import { applyTheme } from "./ui/adapters/browser/theme";

const platform = isTauriRuntime() ? createTauriPlatformAdapter() : createBrowserPlatformAdapter();

setPlatformAdapter(platform);
setTextEditorHostFactory(createCodeMirrorTextEditorHostFactory());

// Theme resolution is a browser bootstrap concern. Apply it before Vue mounts so every route,
// including login/error states, follows the stored preference and OS color-scheme immediately.
let appliedTheme = displayPreferencesService.getState().theme;
applyTheme(appliedTheme);
displayPreferencesService.subscribe(() => {
  const nextTheme = displayPreferencesService.getState().theme;

  if (nextTheme !== appliedTheme) {
    appliedTheme = nextTheme;
    applyTheme(nextTheme);
  }
});
