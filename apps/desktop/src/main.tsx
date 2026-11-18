import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./themes/tokens.css";
import "./themes/deep-field.css";
import "./themes/saturn.css";
import "./themes/nebula.css";
import "./themes/lunar.css";
import "./styles.css";
import "./themes/ui-polish.css";
import { applyUiStyle, readUiStyle } from "./ui-styles";

applyUiStyle(readUiStyle());

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);