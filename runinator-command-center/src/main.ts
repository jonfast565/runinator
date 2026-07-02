import { createApp } from "vue";
import { createPinia } from "pinia";
import { setCommandRuntime } from "./core/api/runtime";
import { tauriCommandRuntime } from "./ui/adapters/tauri/command-runtime";
import App from "./App.vue";
import "@vue-flow/core/dist/style.css";
import "./styles/base.css";
import "./styles/layout.css";
import "./styles/buttons.css";
import "./styles/tables.css";
import "./styles/forms.css";
import "./styles/badges.css";
import "./styles/workflow.css";
import "./styles/modal.css";

setCommandRuntime(tauriCommandRuntime);

createApp(App).use(createPinia()).mount("#app");
