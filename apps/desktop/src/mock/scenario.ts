export type MockScenario =
  | "home"
  | "home-empty"
  | "workspace-prepare"
  | "workspace-preview"
  | "device-lan"
  | "device-cloud-in"
  | "device-cloud-out";

export interface MockFlags {
  scenario: MockScenario;
  cloudSignedIn: boolean;
  hostConnected: boolean;
  sliced: boolean;
  models: boolean;
  recents: boolean;
  /** Show a sample HMS alert on the device pane. */
  hms: boolean;
}

const DEFAULT: MockFlags = {
  scenario: "workspace-preview",
  cloudSignedIn: false,
  hostConnected: true,
  sliced: true,
  models: true,
  recents: true,
  hms: false,
};

export function readMockFlags(): MockFlags {
  const params = new URLSearchParams(window.location.search);
  const scenario = (params.get("mock") as MockScenario | null) ?? DEFAULT.scenario;
  const flags: MockFlags = { ...DEFAULT, scenario };
  if (params.get("cloud") === "in") flags.cloudSignedIn = true;
  if (params.get("cloud") === "out") flags.cloudSignedIn = false;
  if (params.get("lan") === "0") flags.hostConnected = false;
  if (params.get("slice") === "0") flags.sliced = false;
  if (params.get("models") === "0") flags.models = false;
  if (params.get("recents") === "0") flags.recents = false;
  if (params.get("hms") === "1") flags.hms = true;

  switch (scenario) {
    case "home":
    case "home-empty":
      flags.models = false;
      flags.sliced = false;
      flags.recents = scenario !== "home-empty";
      break;
    case "workspace-prepare":
      flags.sliced = false;
      flags.models = true;
      break;
    case "workspace-preview":
      flags.sliced = true;
      flags.models = true;
      break;
    case "device-lan":
      flags.hostConnected = true;
      flags.cloudSignedIn = false;
      flags.models = true;
      flags.sliced = true;
      break;
    case "device-cloud-in":
      flags.hostConnected = false;
      flags.cloudSignedIn = true;
      flags.models = true;
      flags.sliced = true;
      break;
    case "device-cloud-out":
      flags.hostConnected = false;
      flags.cloudSignedIn = false;
      flags.models = true;
      flags.sliced = params.get("slice") !== "0";
      break;
    default:
      break;
  }
  return flags;
}