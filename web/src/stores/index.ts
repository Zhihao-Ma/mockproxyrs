import { createPinia } from "pinia";

const pinia = createPinia();

export function setupStore(app: import("vue").App) {
  app.use(pinia);
}

export { pinia };
export { useLayoutStore } from "./layout";
