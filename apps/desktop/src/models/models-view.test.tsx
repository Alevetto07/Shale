import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ModelsDiscover, type ModelsDiscoverProps } from "./ModelsView";

function props(overrides: Partial<ModelsDiscoverProps> = {}): ModelsDiscoverProps {
  return {
    library: "makerworld",
    onLibrary: vi.fn(),
    query: "clip",
    onQuery: vi.fn(),
    sort: "relevant",
    onSort: vi.fn(),
    onSearch: vi.fn(),
    searching: false,
    error: null,
    hits: [
      {
        library: "makerworld",
        id: "1400373",
        title: "Cable clip",
        creator: "Ada Maker",
        creator_url: "https://makerworld.com/en/@ada",
        licence: "Creative Commons Attribution-NonCommercial 4.0",
        thumbnail_url: null,
        page_url: "https://makerworld.com/en/models/1400373-cable-clip",
        likes: 12,
        downloads: 40,
        nsfw: false,
      },
    ],
    total: 1,
    page: 1,
    onPage: vi.fn(),
    listing: {
      card: {
        library: "makerworld",
        id: "1400373",
        title: "Cable clip",
        creator: "Ada Maker",
        creator_url: null,
        licence: "Creative Commons Attribution-NonCommercial 4.0",
        thumbnail_url: null,
        page_url: "https://makerworld.com/en/models/1400373-cable-clip",
        likes: 12,
        downloads: 40,
        nsfw: false,
      },
      summary: "A clip for a desk edge.",
      profiles: [{ id: "4242", name: "0.20 mm A1", printer: "Bambu Lab A1" }],
      files: [],
      remote_model_id: "US300293b2858c3b",
    },
    listingId: "1400373",
    onSelect: vi.fn(),
    profileId: "4242",
    onProfile: vi.fn(),
    fileId: null,
    onFile: vi.fn(),
    onOpen: vi.fn(),
    opening: false,
    link: "",
    onLink: vi.fn(),
    onOpenLink: vi.fn(),
    tokenConfigured: false,
    tokenDraft: "",
    onTokenDraft: vi.fn(),
    onSaveToken: vi.fn(),
    onClearToken: vi.fn(),
    onOpenAccounts: vi.fn(),
    onOpenListing: vi.fn(),
    ...overrides,
  };
}

describe("ModelsDiscover", () => {
  it("shows the creator, the licence, and the print profile", () => {
    const onOpen = vi.fn();
    render(<ModelsDiscover {...props({ onOpen })} />);
    expect(screen.getAllByText("By Ada Maker").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Creative Commons Attribution-NonCommercial 4.0").length).toBeGreaterThan(0);
    expect(screen.getByText("0.20 mm A1")).toBeTruthy();
    expect(screen.getByText("Bambu Lab A1")).toBeTruthy();
    fireEvent.click(within(document.querySelector(".models-detail") as HTMLElement).getByRole("button", { name: "Open in Shale" }));
    expect(onOpen).toHaveBeenCalledOnce();
    expect(document.body.textContent?.toLowerCase().includes("deep field")).toBe(false);
  });

  it("asks for a Thingiverse app token and links to the developer page", () => {
    render(<ModelsDiscover {...props({ library: "thingiverse", hits: [], listing: null, listingId: null })} />);
    expect(screen.getByRole("heading", { name: "Thingiverse needs an app token" })).toBeTruthy();
    const link = screen.getByRole("link", { name: "Thingiverse developer page" });
    expect(link.getAttribute("href")).toBe("https://www.thingiverse.com/developers");
    expect(screen.getByLabelText("Thingiverse app token")).toBeTruthy();
  });
});
