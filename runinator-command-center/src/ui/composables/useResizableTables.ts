import { onBeforeUnmount, onMounted } from "vue";

const DEFAULT_COLUMN_WIDTH = 160;
const MIN_COLUMN_WIDTH = 80;
const RESIZE_STEP = 16;

interface ResizableTable {
  columns: HTMLTableColElement[];
  headers: HTMLTableCellElement[];
}

const tables = new WeakMap<HTMLTableElement, ResizableTable>();

function headerCells(table: HTMLTableElement): HTMLTableCellElement[] {
  const row = table.tHead?.rows.item(0);

  if (!row || row.cells.length < 2) {
    return [];
  }

  const headers = Array.from(row.cells);

  // Grouped headings need more than one logical column. Every current data table has a flat
  // heading row, so that stays the universal, reliable resize contract.
  return headers.every((cell) => cell.tagName === "TH" && cell.colSpan === 1) ? headers : [];
}

function columnWidth(column: HTMLTableColElement): number {
  return Number.parseFloat(column.style.width) || DEFAULT_COLUMN_WIDTH;
}

function updateHandleValue(header: HTMLTableCellElement, width: number) {
  header
    .querySelector<HTMLElement>(".table-column-resize-handle")
    ?.setAttribute("aria-valuenow", String(Math.round(width)));
}

function resizeColumn(table: ResizableTable, index: number, delta: number) {
  const neighbor = index === table.columns.length - 1 ? index - 1 : index + 1;

  if (neighbor < 0) {
    return;
  }

  const targetWidth = columnWidth(table.columns[index]);
  const neighborWidth = columnWidth(table.columns[neighbor]);
  const combinedWidth = targetWidth + neighborWidth;
  const nextTargetWidth = Math.max(
    MIN_COLUMN_WIDTH,
    Math.min(combinedWidth - MIN_COLUMN_WIDTH, targetWidth + delta),
  );

  if (nextTargetWidth === targetWidth) {
    return;
  }

  table.columns[index].style.width = `${String(nextTargetWidth)}px`;
  table.columns[neighbor].style.width = `${String(combinedWidth - nextTargetWidth)}px`;
  updateHandleValue(table.headers[index], nextTargetWidth);
  updateHandleValue(table.headers[neighbor], combinedWidth - nextTargetWidth);
}

function resizeLabel(header: HTMLTableCellElement): string {
  return header.textContent.trim() || "unnamed";
}

function addResizeHandle(table: ResizableTable, header: HTMLTableCellElement, index: number) {
  if (header.querySelector(".table-column-resize-handle")) {
    return;
  }

  const handle = document.createElement("span");
  handle.className = "table-column-resize-handle";
  handle.tabIndex = 0;
  handle.setAttribute("role", "separator");
  handle.setAttribute("aria-orientation", "vertical");
  handle.setAttribute("aria-label", `Resize ${resizeLabel(header)} column`);
  handle.setAttribute("aria-valuemin", String(MIN_COLUMN_WIDTH));
  handle.setAttribute("aria-valuenow", String(Math.round(columnWidth(table.columns[index]))));

  handle.addEventListener("click", (event) => {
    event.stopPropagation();
  });
  handle.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    const direction = event.key === "ArrowRight" ? 1 : -1;
    resizeColumn(table, index, direction * (event.shiftKey ? RESIZE_STEP * 3 : RESIZE_STEP));
  });
  handle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();

    const startX = event.clientX;
    const startWidth = columnWidth(table.columns[index]);

    const onPointerMove = (moveEvent: PointerEvent) => {
      const alreadyApplied = columnWidth(table.columns[index]) - startWidth;
      resizeColumn(table, index, moveEvent.clientX - startX - alreadyApplied);
    };

    const finishResize = () => {
      document.body.classList.remove("table-column-resize-active");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", finishResize);
      window.removeEventListener("pointercancel", finishResize);
    };

    document.body.classList.add("table-column-resize-active");
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", finishResize);
    window.addEventListener("pointercancel", finishResize);
  });

  header.append(handle);
}

function makeResizable(table: HTMLTableElement) {
  const headers = headerCells(table);

  if (!headers.length) {
    return;
  }

  let resizable = tables.get(table);

  if (resizable?.columns.length !== headers.length) {
    table.querySelector(":scope > colgroup[data-resizable-columns]")?.remove();
    const colgroup = document.createElement("colgroup");
    colgroup.dataset.resizableColumns = "";
    const columns = headers.map((header) => {
      const column = document.createElement("col");
      const measuredWidth = Math.round(header.getBoundingClientRect().width);
      column.style.width = `${String(measuredWidth || DEFAULT_COLUMN_WIDTH)}px`;
      colgroup.append(column);
      return column;
    });

    table.insertBefore(colgroup, table.tHead);
    resizable = { columns, headers };
    tables.set(table, resizable);
    table.classList.add("resizable-table");
    table.style.tableLayout = "fixed";
  } else {
    resizable.headers = headers;
  }

  headers.forEach((header, index) => {
    addResizeHandle(resizable, header, index);
  });
}

/** Apply one consistent, accessible column-resize interaction to every DataTable-rendered table. */
export function useResizableTables() {
  let observer: MutationObserver | undefined;
  let animationFrame = 0;
  const pendingTables = new Set<HTMLTableElement>();

  const schedule = (table: HTMLTableElement) => {
    pendingTables.add(table);

    if (animationFrame) {
      return;
    }

    animationFrame = window.requestAnimationFrame(() => {
      pendingTables.forEach(makeResizable);
      pendingTables.clear();
      animationFrame = 0;
    });
  };

  onMounted(() => {
    document.querySelectorAll<HTMLTableElement>("table").forEach(schedule);
    observer = new MutationObserver((records) => {
      for (const record of records) {
        const table = record.target instanceof Element ? record.target.closest("table") : null;

        if (table instanceof HTMLTableElement) {
          schedule(table);
        }

        record.addedNodes.forEach((node) => {
          if (!(node instanceof Element)) {
            return;
          }

          if (node instanceof HTMLTableElement) {
            schedule(node);
          }

          node.querySelectorAll<HTMLTableElement>("table").forEach(schedule);
        });
      }
    });
    observer.observe(document.body, { childList: true, subtree: true });
  });

  onBeforeUnmount(() => {
    observer?.disconnect();
    window.cancelAnimationFrame(animationFrame);
    pendingTables.clear();
  });
}
