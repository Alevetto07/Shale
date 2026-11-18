import { DemoProvider } from "../../lib/demo-context";
import { HeroLayerCanvas } from "./HeroLayerCanvas";

export default function HeroPrintSceneRoot() {
  return (
    <DemoProvider>
      <HeroLayerCanvas />
    </DemoProvider>
  );
}