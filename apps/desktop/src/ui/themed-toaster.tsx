import { Toaster } from "sonner";
import type { UiStyleId } from "../ui-styles";

/** Sonner mounted once; appearance tracks Shale tokens on `data-ui-theme`. */
export function ThemedToaster({ uiStyle }: { uiStyle: UiStyleId }) {
  const sonnerTheme = uiStyle === "lunar" ? "light" : "dark";

  return (
    <Toaster
      position="bottom-right"
      closeButton={false}
      theme={sonnerTheme}
      offset={{ bottom: 88, right: 20 }}
      mobileOffset={{ bottom: 72, right: 16 }}
      toastOptions={{
        unstyled: true,
        classNames: {
          toast: "shale-toast",
          title: "shale-toast__title",
          description: "shale-toast__description",
          actionButton: "shale-toast__action",
          cancelButton: "shale-toast__cancel",
          closeButton: "shale-toast__close",
        },
      }}
    />
  );
}