import { listen } from "@tauri-apps/api/event";
import { trace } from "@tauri-apps/plugin-log";
import { useEffect } from "react";
import { toast } from "sonner";

function useIncomingApiRequest() {
  useEffect(() => {
    const toastId = toast("Incoming API Request");
    trace(`useIncomingApiRequest useEffect`);

    const unlisten = listen<{ buffer: number[] }>(
      "incoming_api_request",
      ({ payload }) => {
        trace(`useIncomingApiRequest listen ${payload}`);
        toast.success(`Buffer received: [${payload.buffer}]`, {
          id: toastId,
        });
      }
    );

    return () => {
      trace(`useChannel return`);
      unlisten.then((f) => f());
    };
  }, []);
}

export default useIncomingApiRequest;
