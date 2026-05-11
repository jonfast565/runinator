import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import "@vue-flow/core/dist/style.css";
import "./styles/base.css";
import "./styles/layout.css";
import "./styles/tables.css";
import "./styles/forms.css";
import "./styles/badges.css";
import "./styles/workflow.css";
import "./styles/modal.css";

createApp(App).use(createPinia()).mount("#app");
