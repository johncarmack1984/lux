import { QueryClient } from "@tanstack/react-query";

/**
 * The app's single QueryClient, exported as a module singleton rather than
 * created inside a component so non-React code can reach the cache too — the
 * channel-value setter in `lib/actions` pushes the buffer the backend returns
 * straight into `["buffer"]`, since the `bufferSet` event that would normally
 * carry it doesn't reach the webview on iOS.
 */
export const queryClient = new QueryClient();
