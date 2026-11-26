import { useState } from "react";
import type { Suggestion } from "./api";
import { Button } from "./ui/button";

interface SuggestionsProps {
  items: Suggestion[];
  onApply: (item: Suggestion) => void;
  onDismiss: (item: Suggestion) => void;
  onNever: (item: Suggestion) => void;
}

export function Suggestions({ items, onApply, onDismiss, onNever }: SuggestionsProps) {
  const warnings = items.filter((item) => item.severity === "warning");
  const tips = items.filter((item) => item.severity !== "warning");
  const [open, setOpen] = useState(warnings.length > 0);
  const [tipsOpen, setTipsOpen] = useState(false);
  if (items.length === 0) return null;
  if (!open) {
    return (
      <button type="button" className="suggest-chip" onClick={() => setOpen(true)}>
        {items.length === 1 ? "1 suggestion" : `${items.length} suggestions`}
      </button>
    );
  }
  return (
    <section className="suggest-panel" aria-label="Suggestions">
      <div className="suggest-head">
        <span>Suggestions</span>
        <Button variant="ghost" size="sm" onClick={() => setOpen(false)}>
          Hide
        </Button>
      </div>
      {warnings.map((item) => (
        <SuggestionCard key={item.id} item={item} onApply={onApply} onDismiss={onDismiss} onNever={onNever} />
      ))}
      {tips.length > 0 && (
        <button type="button" className="suggest-chip" onClick={() => setTipsOpen((value) => !value)}>
          {tipsOpen ? "Hide tips" : tips.length === 1 ? "1 tip" : `${tips.length} tips`}
        </button>
      )}
      {tipsOpen &&
        tips.map((item) => (
          <SuggestionCard key={item.id} item={item} onApply={onApply} onDismiss={onDismiss} onNever={onNever} />
        ))}
    </section>
  );
}

function SuggestionCard({
  item,
  onApply,
  onDismiss,
  onNever,
}: {
  item: Suggestion;
  onApply: (item: Suggestion) => void;
  onDismiss: (item: Suggestion) => void;
  onNever: (item: Suggestion) => void;
}) {
  const [why, setWhy] = useState(false);
  return (
    <article className="suggest-card" data-severity={item.severity}>
      <p className="suggest-title">{item.title}</p>
      <div className="suggest-actions">
        {item.fix && (
          <Button variant="primary" size="sm" onClick={() => onApply(item)}>
            {item.fix.label || "Apply"}
          </Button>
        )}
        <Button variant="ghost" size="sm" onClick={() => onDismiss(item)}>
          Dismiss
        </Button>
        <Button variant="ghost" size="sm" onClick={() => setWhy((value) => !value)}>
          Why
        </Button>
      </div>
      {why && (
        <div className="suggest-why">
          <p>{item.reason}</p>
          {item.dismissible && (
            <Button variant="ghost" size="sm" onClick={() => onNever(item)}>
              Don&apos;t suggest this again
            </Button>
          )}
        </div>
      )}
    </article>
  );
}
