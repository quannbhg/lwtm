// Vertical virtualization for tables with a fixed row height:
// only the visible rows (+ an overscan buffer) are rendered, and two spacer rows stand in for the rest.
//
//   const vl = createVirtual(34);
//   <div class="table-wrap" use:vl.action> ... <tr style="height:{r.padTop}px"> ...
//   const r = vl.range(rows.length);  rows.slice(r.start, r.end)

export function createVirtual(rowH = 34, overscan = 10) {
  const v = $state({ top: 0, height: 600, node: null });

  function action(node) {
    v.node = node;
    v.height = node.clientHeight;
    const onScroll = () => (v.top = node.scrollTop);
    const ro = new ResizeObserver(() => (v.height = node.clientHeight));
    node.addEventListener("scroll", onScroll, { passive: true });
    ro.observe(node);
    return {
      destroy() {
        node.removeEventListener("scroll", onScroll);
        ro.disconnect();
        v.node = null;
      },
    };
  }

  function range(count) {
    const start = Math.max(0, Math.floor(v.top / rowH) - overscan);
    const end = Math.min(count, Math.ceil((v.top + v.height) / rowH) + overscan);
    return { start, end, padTop: start * rowH, padBottom: Math.max(0, count - end) * rowH };
  }

  /** Scroll so that row `index` sits in the middle of the viewport */
  function scrollTo(index) {
    v.node?.scrollTo({ top: Math.max(0, index * rowH - v.height / 2) });
  }

  return { action, range, scrollTo };
}
