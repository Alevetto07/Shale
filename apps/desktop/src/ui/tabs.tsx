import * as TabsPrimitive from "@radix-ui/react-tabs";
import { forwardRef, type ComponentPropsWithoutRef } from "react";
import { cn } from "./cn";

export const Tabs = TabsPrimitive.Root;

export const TabsList = forwardRef<HTMLDivElement, ComponentPropsWithoutRef<typeof TabsPrimitive.List>>(
  function TabsList({ className, ...props }, ref) {
    return <TabsPrimitive.List ref={ref} className={cn("ui-tabs__list", className)} {...props} />;
  },
);

export const TabsTrigger = forwardRef<HTMLButtonElement, ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger>>(
  function TabsTrigger({ className, ...props }, ref) {
    return <TabsPrimitive.Trigger ref={ref} className={cn("ui-tabs__trigger", className)} {...props} />;
  },
);

export const TabsContent = forwardRef<HTMLDivElement, ComponentPropsWithoutRef<typeof TabsPrimitive.Content>>(
  function TabsContent({ className, ...props }, ref) {
    return <TabsPrimitive.Content ref={ref} className={cn("ui-tabs__content", className)} {...props} />;
  },
);
