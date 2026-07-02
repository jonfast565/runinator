import { hoverTooltip, type Tooltip, type EditorView } from "@codemirror/view";
import { hoverWdl } from "../api/commandCenterApi";
import type {
  ProviderMetadata,
  WdlHoverRequest,
  WdlHoverResponse,
  WdlSettingRef,
} from "../types/models";
import { utf16OffsetToUtf8ByteOffset, utf8ByteOffsetToUtf16Offset } from "./wdl-completion";

export function wdlHoverTooltip(
  providers: () => ProviderMetadata[],
  settings: () => WdlSettingRef[] = () => [],
) {
  return hoverTooltip(async (view: EditorView, pos: number): Promise<Tooltip | null> => {
    const source = view.state.doc.toString();
    const request = buildWdlHoverRequest(source, pos, providers(), settings());
    let response: WdlHoverResponse | null;

    try {
      response = await hoverWdl(request);
    } catch {
      return null;
    }

    if (!response) {
      return null;
    }

    return hoverResponseToTooltip(source, response);
  });
}

export function buildWdlHoverRequest(
  source: string,
  cursorOffset: number,
  providers: ProviderMetadata[],
  settings: WdlSettingRef[] = [],
): WdlHoverRequest {
  return {
    source,
    cursor_byte: utf16OffsetToUtf8ByteOffset(source, cursorOffset),
    providers,
    settings,
  };
}

function hoverResponseToTooltip(source: string, response: WdlHoverResponse): Tooltip {
  const from = utf8ByteOffsetToUtf16Offset(source, response.range_start_byte);
  const to = utf8ByteOffsetToUtf16Offset(source, response.range_end_byte);
  return {
    pos: from,
    end: Math.max(to, from + 1),
    above: true,
    create() {
      const dom = document.createElement("div");
      dom.className = "wdl-hover";

      const title = document.createElement("div");
      title.className = "wdl-hover-title";
      title.textContent = response.title;
      dom.appendChild(title);

      const meta = document.createElement("div");
      meta.className = "wdl-hover-meta";
      meta.textContent = [response.kind, response.detail].filter(Boolean).join(" · ");
      dom.appendChild(meta);

      if (response.documentation) {
        const docs = document.createElement("div");
        docs.className = "wdl-hover-docs";
        docs.textContent = response.documentation;
        dom.appendChild(docs);
      }

      return { dom };
    },
  };
}
