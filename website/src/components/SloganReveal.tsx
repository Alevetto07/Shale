import { useEffect, useRef } from "react";

interface Props {
  text: string;
  className?: string;
  as?: "p" | "h2";
  /** Animate on first paint (hero), not on scroll */
  immediate?: boolean;
}

export function SloganReveal({
  text,
  className = "",
  as = "p",
  immediate = false,
}: Props) {
  const ref = useRef<HTMLParagraphElement | HTMLHeadingElement>(null);

  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduced) {
      node.classList.add("slogan-visible");
      return;
    }
    if (immediate) {
      const id = requestAnimationFrame(() => node.classList.add("slogan-visible"));
      return () => cancelAnimationFrame(id);
    }
    const io = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          node.classList.add("slogan-visible");
          io.disconnect();
        }
      },
      { threshold: 0.35 },
    );
    io.observe(node);
    return () => io.disconnect();
  }, [immediate]);

  const Tag = as;
  return (
    <Tag ref={ref} className={`slogan-display ${className}`}>
      {text}
    </Tag>
  );
}