/**
 * Mount the `keeper-media` panel into a plain DOM node, for a CodeMirror
 * widget to own.
 *
 * `mountNoteWidget`'s shape and its reason: the React boundary lives here so
 * that `media-block.ts` — which `live-preview.ts` imports statically — carries
 * no React import at all (NFR-27). `update` re-renders the same root with a new
 * body, so a marker edit resolves in place and the player keeps playing.
 */
import { createRoot } from "react-dom/client";
import { MediaBlockPanel } from "../media-block-panel";
import type { MediaBlockMountArgs, MountedMediaBlock } from "./media-block";

export function mountMediaBlock(
  container: HTMLElement,
  args: MediaBlockMountArgs,
): MountedMediaBlock {
  const root = createRoot(container);
  root.render(<MediaBlockPanel {...args} />);
  return {
    update: (next) => {
      root.render(<MediaBlockPanel {...next} />);
    },
    unmount: () => {
      root.unmount();
    },
  };
}
