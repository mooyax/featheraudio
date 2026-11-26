"""Writes dist/featheraudio-demo.html: the demo page with its script and WebAssembly inside,
so it works as a single file (no server needed)."""
import base64, pathlib
d = pathlib.Path(__file__).parent / "dist"
html = (d / "index.html").read_text(encoding="utf-8")
js = (d / "featheraudio_web.js").read_text(encoding="utf-8")
wasm = base64.b64encode((d / "featheraudio_web_bg.wasm").read_bytes()).decode()
html = html.replace('<script src="featheraudio_web.js"></script>', "<script>\n" + js + "\n</script>")
html = html.replace('const WASM = "featheraudio_web_bg.wasm";', 'const WASM = Uint8Array.from(atob("' + wasm + '"), (c) => c.charCodeAt(0));')
(d / "featheraudio-demo.html").write_text(html, encoding="utf-8")
print("wrote", d / "featheraudio-demo.html", len(html) // 1024, "KB")
