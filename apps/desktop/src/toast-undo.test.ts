import { describe, expect, it, vi, beforeEach } from "vitest";
import { toast } from "sonner";
import { toastUndo } from "./ui/toast-undo";

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { dismiss: vi.fn() }),
}));

describe("toastUndo", () => {
  beforeEach(() => {
    vi.mocked(toast).mockClear();
    vi.mocked(toast.dismiss).mockClear();
  });

  it("registers an undo action on the toast", () => {
    const onUndo = vi.fn();
    toastUndo("Part removed", onUndo, { id: "remove-part" });
    expect(toast).toHaveBeenCalledWith(
      "Part removed",
      expect.objectContaining({
        id: "remove-part",
        action: expect.objectContaining({ label: "Undo" }),
      }),
    );
    const options = vi.mocked(toast).mock.calls[0]?.[1] as unknown as {
      action: { onClick: (e: { preventDefault: () => void }) => void };
    };
    const event = { preventDefault: vi.fn() };
    options.action.onClick(event);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(onUndo).toHaveBeenCalled();
    expect(toast.dismiss).toHaveBeenCalledWith("remove-part");
  });
});