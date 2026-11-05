import { defineConfig } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";

// One view per build: each becomes a self-contained dist/<view>.html that the
// Rust server serves as a ui:// resource.
const input = process.env.INPUT;
if (!input) throw new Error("set INPUT to the view to build (titles | title)");

export default defineConfig({
  plugins: [viteSingleFile()],
  build: {
    outDir: "dist",
    emptyOutDir: false,
    rollupOptions: { input: `${input}.html` },
  },
});
