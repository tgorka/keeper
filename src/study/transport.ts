export interface StudyTransport {
  stop(): void;
}

/** A dedicated study document only. Installed before importing the SDK, never in the app. */
export function installStudyTransport(host: string, onFailure: () => void): StudyTransport {
  const destination = new URL(host);
  if (
    destination.protocol !== "https:" ||
    destination.username ||
    destination.password ||
    destination.pathname !== "/" ||
    destination.search ||
    destination.hash
  ) {
    throw new Error("Invalid study destination");
  }
  const originalFetch = window.fetch.bind(window);
  const activeRequests = new Set<AbortController>();
  let stopped = false;
  let requests = 0;
  const stop = () => {
    stopped = true;
    for (const request of activeRequests) request.abort();
    activeRequests.clear();
    // Keep the fence closed until document destruction: late retries cannot regain fetch.
  };
  const refuse = () => {
    if (!stopped) {
      stop();
      onFailure();
    }
    return new DOMException("Study request refused", "AbortError");
  };

  window.fetch = async (input, init) => {
    let url: URL;
    try {
      url = new URL(input instanceof Request ? input.url : String(input), window.location.href);
    } catch {
      throw refuse();
    }
    // Tauri IPC is local, and must remain usable to clear the backend disclosure.
    if (url.protocol === "ipc:" || url.hostname === "ipc.localhost")
      return originalFetch(input, init);
    if (
      stopped ||
      url.origin !== destination.origin ||
      ++requests > 600 ||
      activeRequests.size >= 4
    ) {
      throw refuse();
    }
    // No scripts, arbitrary endpoints, assets, flag evaluation, or cross-host redirects.
    if (!/^\/(?:e\/?|s\/?|i\/v0\/e\/?|array\/phc_[A-Za-z0-9_-]+\/config)$/.test(url.pathname)) {
      throw refuse();
    }
    const controller = new AbortController();
    activeRequests.add(controller);
    const abort = () => controller.abort();
    const upstream = init?.signal ?? (input instanceof Request ? input.signal : null);
    upstream?.addEventListener("abort", abort, { once: true });
    if (upstream?.aborted) abort();
    const deadline = setTimeout(abort, 5_000);
    try {
      const response = await originalFetch(input, {
        ...init,
        signal: controller.signal,
        credentials: "omit",
        referrerPolicy: "no-referrer",
        redirect: "error",
        keepalive: false,
      });
      if (!response.ok) throw refuse();
      return response;
    } catch {
      throw refuse();
    } finally {
      clearTimeout(deadline);
      upstream?.removeEventListener("abort", abort);
      activeRequests.delete(controller);
    }
  };
  // Beacon cannot be recalled or aborted. Never allow the SDK's unload/shutdown fallback.
  Object.defineProperty(window.navigator, "sendBeacon", { configurable: true, value: () => true });
  // Explicit fetch transport is the only supported study transport; no XHR fallback can escape.
  const originalOpen = window.XMLHttpRequest.prototype.open;
  const urls = new WeakMap<XMLHttpRequest, string>();
  window.XMLHttpRequest.prototype.open = function (
    method: string,
    url: string | URL,
    async: boolean = true,
    username?: string | null,
    password?: string | null,
  ) {
    urls.set(this, String(url));
    originalOpen.call(this, method, url, async, username, password);
  };
  const originalSend = window.XMLHttpRequest.prototype.send;
  window.XMLHttpRequest.prototype.send = function (body) {
    const url = new URL(urls.get(this) ?? "", window.location.href);
    if (url.protocol === "ipc:" || url.hostname === "ipc.localhost")
      return originalSend.call(this, body);
    throw refuse();
  };
  return { stop };
}
