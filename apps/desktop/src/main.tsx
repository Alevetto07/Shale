import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { MotionConfig } from "motion/react";
import { App } from "./App";
import "./themes/tokens.css";
import "./themes/deep-field.css";
import "./themes/saturn.css";
import "./themes/nebula.css";
import "./themes/lunar.css";
import "./styles.css";
import "./themes/ui-polish.css";
import "./ui/components.css";
import "./themes/studio.css";
import { applyUiStyle, readUiStyle } from "./ui-styles";
import { springPanel } from "./ui/springs";
applyUiStyle(readUiStyle());

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <MotionConfig transition={springPanel} reducedMotion="user">
      <App />
    </MotionConfig>
  </StrictMode>,
);