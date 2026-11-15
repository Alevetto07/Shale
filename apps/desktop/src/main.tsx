import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./themes/tokens.css";
import "./themes/deep-field.css";
import "./styles.css";
import "./themes/ui-polish.css";
import { UI_THEME_ID } from "./theme";

document.documentElement.dataset.uiTheme = UI_THEME_ID;

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
