import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CommandPalette, type CommandPaletteItem } from "./ui";

const fixture: CommandPaletteItem[] = [
  {
    id: "new",
    group: "Project",
    label: "New project",
    keywords: ["create"],
    onSelect: vi.fn(),
  },
  {
    id: "open",
    group: "Project",
    label: "Open project",
    keywords: ["file"],
    onSelect: vi.fn(),
  },
  {
    id: "home",
    group: "Navigate",
    label: "Go to Home",
    keywords: ["start"],
    onSelect: vi.fn(),
  },
];

describe("CommandPalette", () => {
  afterEach(() => {
    cleanup();
  });

  it("filters commands with fuzzy search", () => {
    render(<CommandPalette open items={fixture} onOpenChange={() => undefined} />);
    const input = screen.getByPlaceholderText("Search commands…");
    fireEvent.change(input, { target: { value: "open" } });
    expect(screen.getByText("Open project")).toBeTruthy();
    expect(screen.queryByText("New project")).toBeNull();
    expect(screen.queryByText("Go to Home")).toBeNull();
  });

  it("closes on Ctrl+K", () => {
    const onOpenChange = vi.fn();
    render(<CommandPalette open items={fixture} onOpenChange={onOpenChange} />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("shows grouped sections in the fixture", () => {
    render(<CommandPalette open items={fixture} onOpenChange={() => undefined} />);
    expect(screen.getByText("New project")).toBeTruthy();
    expect(screen.getByText("Go to Home")).toBeTruthy();
  });
});