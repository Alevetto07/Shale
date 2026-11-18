import { DemoProvider } from "../../lib/demo-context";
import { HeroPrintScene } from "./HeroPrintScene";

export default function HeroPrintSceneRoot() {
  return (
    <DemoProvider>
      <HeroPrintScene />
    </DemoProvider>
  );
}