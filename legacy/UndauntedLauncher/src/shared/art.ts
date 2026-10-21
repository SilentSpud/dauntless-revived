// The host's art reaches the page only through the launcher's own `dr-art` scheme (MIGRATION_PLAN.md L37,
// L41). The backend names each image `dr-art://bg/<sha256>`. Electron and WebKitGTK load that URL as it
// is; WebView2 serves custom schemes only as `http://<scheme>.localhost/`, so the same image is
// `http://dr-art.localhost/bg/<sha256>` there. Anything else is not art and is never loaded.

const ART_URL = /^dr-art:\/\/bg\/([0-9a-f]{64})$/;

/** The URL to load an art image from in this webview, or null when `url` is not one of the launcher's. */
export function artSource(url: string, webView2: boolean): string | null {
  const id = ART_URL.exec(url)?.[1];
  if (!id) return null;
  return webView2 ? `http://dr-art.localhost/bg/${id}` : url;
}
