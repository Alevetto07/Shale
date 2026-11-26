import { animate, motion, useMotionValue, useTransform, type MotionValue } from "motion/react";
import { createContext, useContext, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { sidebarSpan, shellShift, springPanel } from "./motion";
import { useReducedMotion } from "./use-reduced-motion";

type SidebarChrome = { collapsed: boolean; settled: boolean };

const SidebarChromeContext = createContext<SidebarChrome | null>(null);

/** Applies the sidebar's slide as a DOM transform, with no pointer tracking and no React render. */
function ShiftedMain({
  shift,
  frozen,
  children,
}: {
  shift: MotionValue<number>;
  frozen: boolean;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) return;
    if (frozen) {
      node.style.transform = "";
      return;
    }
    const apply = (value: number) => {
      node.style.transform = value === 0 ? "" : `translateX(${value}px)`;
    };
    apply(shift.get());
    return shift.on("change", apply);
  }, [shift, frozen]);
  return (
    <div ref={ref} className="app-main__shift">
      {children}
    </div>
  );
}

export function useSidebarChrome(): SidebarChrome | null {
  return useContext(SidebarChromeContext);
}

/**
 * Sidebar clip animates width out of flow. The main column's margin changes
 * once per toggle; a transform (`shellShift`) tracks the clip so the 3D
 * viewport is not laid out on every frame.
 */
export function ShellFrame({
  collapsed,
  sidebar,
  children,
}: {
  collapsed: boolean;
  sidebar: ReactNode;
  children: ReactNode;
}) {
  const reduce = useReducedMotion();
  const target = sidebarSpan(collapsed);
  const width = useMotionValue(target);
  const targetRef = useRef(target);
  targetRef.current = target;
  const shift = useTransform(width, (value) => shellShift(value, targetRef.current));
  const [settled, setSettled] = useState(true);

  useLayoutEffect(() => {
    if (reduce || width.get() === target) {
      width.set(target);
      setSettled(true);
      return;
    }
    setSettled(false);
    const controls = animate(width, target, {
      ...springPanel,
      onComplete: () => setSettled(true),
    });
    return () => controls.stop();
  }, [reduce, target, width]);

  return (
    <SidebarChromeContext.Provider value={{ collapsed, settled }}>
      <div
        className="app-shell"
        data-sidebar={collapsed ? "rail" : "full"}
        data-sidebar-settled={settled ? "1" : "0"}
      >
        <motion.div className="ui-sidebar-clip" style={{ width }} data-collapsed={collapsed ? "1" : "0"}>
          {sidebar}
        </motion.div>
        <div className="app-main" style={{ marginLeft: target }}>
          <ShiftedMain shift={shift} frozen={reduce}>
            {children}
          </ShiftedMain>
        </div>
      </div>
    </SidebarChromeContext.Provider>
  );
}
