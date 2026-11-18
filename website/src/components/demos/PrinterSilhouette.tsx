export function PrinterSilhouette({ kind = "core" }: { kind?: "mini" | "core" | "enclosed" }) {
  const h = kind === "mini" ? 36 : 44;
  const w = kind === "mini" ? 32 : 40;
  return (
    <svg
      className="printer-glyph"
      width={w}
      height={h}
      viewBox="0 0 40 44"
      aria-hidden
    >
      {kind === "enclosed" ? (
        <>
          <rect x="6" y="8" width="28" height="30" rx="3" fill="currentColor" opacity="0.15" />
          <rect x="10" y="14" width="20" height="14" rx="1" fill="none" stroke="currentColor" strokeWidth="1.2" />
          <rect x="8" y="32" width="24" height="4" rx="1" fill="currentColor" opacity="0.35" />
        </>
      ) : kind === "mini" ? (
        <>
          <path
            d="M10 12 h20 v22 a2 2 0 0 1 -2 2 H12 a2 2 0 0 1 -2 -2 Z"
            fill="currentColor"
            opacity="0.12"
          />
          <rect x="12" y="30" width="16" height="3" rx="0.5" fill="currentColor" opacity="0.4" />
          <line x1="20" y1="16" x2="20" y2="26" stroke="currentColor" strokeWidth="1.2" />
        </>
      ) : (
        <>
          <rect x="8" y="10" width="24" height="26" rx="2" fill="currentColor" opacity="0.12" />
          <rect x="11" y="28" width="18" height="5" rx="1" fill="currentColor" opacity="0.35" />
          <path d="M14 16 h12 M14 20 h8" stroke="currentColor" strokeWidth="1" opacity="0.6" />
        </>
      )}
    </svg>
  );
}