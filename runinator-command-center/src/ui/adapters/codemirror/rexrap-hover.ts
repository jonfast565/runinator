import { hoverTooltip, type Tooltip, type EditorView } from "@codemirror/view";
import { rexrapLanguageService } from "../../../core/services";
import type {
  ProviderMetadata,
  RexRapHoverRequest,
  RexRapHoverResponse,
  RexRapSettingRef,
} from "../../../core/domain/models";
import { utf16OffsetToUtf8ByteOffset, utf8ByteOffsetToUtf16Offset } from "./rexrap-completion";

export function rexrapHoverTooltip(
  providers: () => ProviderMetadata[],
  settings: () => RexRapSettingRef[] = () => [],
) {
  return hoverTooltip(async (view: EditorView, pos: number): Promise<Tooltip | null> => {
    const source = view.state.doc.toString();
    const request = buildRexRapHoverRequest(source, pos, providers(), settings());
    let response: RexRapHoverResponse | null;

    try {
      response = await rexrapLanguageService.hover(request);
    } catch {
      return null;
    }

    if (!response) {
      return null;
    }

    return hoverResponseToTooltip(source, response);
  });
}

export function buildRexRapHoverRequest(
  source: string,
  cursorOffset: number,
  providers: ProviderMetadata[],
  settings: RexRapSettingRef[] = [],
): RexRapHoverRequest {
  return {
    source,
    cursor_byte: utf16OffsetToUtf8ByteOffset(source, cursorOffset),
    providers,
    settings,
  };
}

function hoverResponseToTooltip(source: string, response: RexRapHoverResponse): Tooltip {
  const from = utf8ByteOffsetToUtf16Offset(source, response.range_start_byte);
  const to = utf8ByteOffsetToUtf16Offset(source, response.range_end_byte);
  return {
    pos: from,
    end: Math.max(to, from + 1),
    above: true,
    create() {
      const dom = document.createElement("div");
      dom.className = "rexrap-hover";

      const title = document.createElement("div");
      title.className = "rexrap-hover-title";
      title.textContent = response.title;
      dom.appendChild(title);

      const meta = document.createElement("div");
      meta.className = "rexrap-hover-meta";
      meta.textContent = [response.kind, response.detail].filter(Boolean).join(" · ");
      dom.appendChild(meta);

      if (response.documentation) {
        const docs = document.createElement("div");
        docs.className = "rexrap-hover-docs";
        docs.textContent = response.documentation;
        dom.appendChild(docs);
      }

      return { dom };
    },
  };
}
