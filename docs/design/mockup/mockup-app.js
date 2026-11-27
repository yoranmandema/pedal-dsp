// Lovable mockup 'splitpath-signal-studio' — app code extracted from the production bundle
// (assets/routes-1NZ8-ivz.js, prettified). Minifier renamed identifiers; see docs/design/ui-grid-port.md
// for the de-minified name map. Lines 1-251 (lucide icons) and the tailwind-merge runtime are omitted.
// Name map: k=BLOCK_TYPES, A=CATEGORIES, M=CAT_COLOR, re=IN_X(138), P=BUS_X(1110), ie=CANVAS_W(1150),
// F(col)=colCenterX, I(row)=rowCenterY, L=GridCanvas, ae=TrimControl, wt=Knob (NOT to be ported),
// $=makeBlock, Et=App, Dt=Divider, Ot=IconButton, kt=SmallButton.

  O = (e, t, n, r, i = ``) => ({ k: e, min: t, max: n, v: r, unit: i }),
  k = [
    {
      type: `amp`,
      label: `Amp`,
      cat: `amp`,
      icon: m,
      params: [
        O(`Gain`, 0, 10, 3.5),
        O(`Bass`, -12, 12, 0, `dB`),
        O(`Mid`, -12, 12, 0, `dB`),
        O(`Treble`, -12, 12, 0, `dB`),
        O(`Level`, -40, 12, -5, `dB`),
      ],
    },
    {
      type: `cab`,
      label: `Cab`,
      cat: `amp`,
      icon: w,
      params: [
        O(`Mic Pos`, 0, 10, 5),
        O(`Distance`, 0, 10, 2),
        O(`Level`, -40, 12, 0, `dB`),
      ],
    },
    {
      type: `nam`,
      label: `NAM`,
      cat: `amp`,
      icon: _,
      params: [O(`Input`, -20, 20, 0, `dB`), O(`Output`, -20, 20, 0, `dB`)],
    },
    {
      type: `od`,
      label: `Overdrive`,
      cat: `drive`,
      icon: f,
      params: [
        O(`Drive`, 0, 10, 4),
        O(`Tone`, 0, 10, 5),
        O(`Level`, -20, 12, 0, `dB`),
      ],
    },
    {
      type: `eq`,
      label: `Tone Stack`,
      cat: `tone`,
      icon: C,
      params: [
        O(`Low`, -12, 12, 0, `dB`),
        O(`Mid`, -12, 12, 0, `dB`),
        O(`High`, -12, 12, 0, `dB`),
      ],
    },
    {
      type: `comp`,
      label: `Compressor`,
      cat: `dyn`,
      icon: y,
      params: [
        O(`Threshold`, -60, 0, -20, `dB`),
        O(`Ratio`, 1, 20, 4),
        O(`Makeup`, 0, 24, 3, `dB`),
      ],
    },
    {
      type: `gate`,
      label: `Noise Gate`,
      cat: `dyn`,
      icon: v,
      params: [
        O(`Threshold`, -90, 0, -60, `dB`),
        O(`Release`, 1, 500, 80, `ms`),
      ],
    },
    {
      type: `delay`,
      label: `Delay`,
      cat: `time`,
      icon: p,
      params: [
        O(`Time`, 1, 2e3, 380, `ms`),
        O(`Feedback`, 0, 100, 35, `%`),
        O(`Mix`, 0, 100, 25, `%`),
      ],
    },
    {
      type: `reverb`,
      label: `Reverb`,
      cat: `time`,
      icon: S,
      params: [
        O(`Decay`, 0.1, 20, 2.4, `s`),
        O(`Pre-delay`, 0, 200, 20, `ms`),
        O(`Mix`, 0, 100, 30, `%`),
      ],
    },
    {
      type: `chorus`,
      label: `Chorus`,
      cat: `mod`,
      icon: D,
      params: [
        O(`Rate`, 0.1, 10, 1.2, `Hz`),
        O(`Depth`, 0, 100, 50, `%`),
        O(`Mix`, 0, 100, 50, `%`),
      ],
    },
    {
      type: `trem`,
      label: `Tremolo`,
      cat: `mod`,
      icon: D,
      params: [O(`Rate`, 0.1, 20, 5, `Hz`), O(`Depth`, 0, 100, 60, `%`)],
    },
    {
      type: `oct`,
      label: `Octaver`,
      cat: `pitch`,
      icon: te,
      params: [
        O(`Sub`, 0, 100, 50, `%`),
        O(`Up`, 0, 100, 0, `%`),
        O(`Dry`, 0, 100, 100, `%`),
      ],
    },
    {
      type: `pdelay`,
      label: `Pitch Delay`,
      cat: `pitch`,
      icon: te,
      params: [
        O(`Pitch`, -12, 12, 7, `st`),
        O(`Time`, 1, 2e3, 250, `ms`),
        O(`Mix`, 0, 100, 40, `%`),
      ],
    },
    {
      type: `gain`,
      label: `Gain`,
      cat: `util`,
      icon: T,
      params: [O(`Gain`, -40, 12, 0, `dB`)],
    },
  ],
  A = [
    { id: `amp`, label: `Modelling` },
    { id: `drive`, label: `Distortion` },
    { id: `tone`, label: `Tone` },
    { id: `dyn`, label: `Dynamics` },
    { id: `time`, label: `Time` },
    { id: `mod`, label: `Modulation` },
    { id: `pitch`, label: `Pitch` },
    { id: `util`, label: `Utility` },
  ],
  j = (e) => k.find((t) => t.type === e),
  M = {
    amp: `var(--color-cat-amp)`,
    drive: `var(--color-cat-drive)`,
    tone: `var(--color-cat-tone)`,
    dyn: `var(--color-cat-dyn)`,
    time: `var(--color-cat-time)`,
    mod: `var(--color-cat-mod)`,
    pitch: `var(--color-cat-pitch)`,
    util: `var(--color-cat-util)`,
  },
  N = n(),
  re = 138,
  P = 1110,
  ie = 1150,
  F = (e) => 160 + e * 116 + 116 / 2,
  I = (e) => e * 112 + 112 / 2;
function L(e) {
  let t = (0, l.useRef)(null),
    [n, r] = (0, l.useState)(null),
    [i, a] = (0, l.useState)(null),
    [o, s] = (0, l.useState)(null),
    c = (t, n) => e.blocks.find((e) => e.row === t && e.col === n),
    u = (t) => e.blocks.find((e) => e.id === t),
    d = (t) => {
      if (e.channels[t].input) return re;
      let n = e.links.map((e) => u(e.to)).filter((e) => e && e.row === t);
      return n.length ? F(Math.min(...n.map((e) => e.col))) - 92 / 2 : null;
    },
    f = (e) => {
      let n = t.current.getBoundingClientRect();
      return { x: e.clientX - n.left, y: e.clientY - n.top };
    },
    p = (t, n) =>
      t.row !== n.row &&
      n.col > t.col &&
      !e.links.some((e) => e.from === t.id && e.to === n.id),
    m = (e, t, n, r) => {
      let i = Math.max(30, (n - e) * 0.5);
      return `M${e} ${t} C${e + i} ${t}, ${n - i} ${r}, ${n} ${r}`;
    };
  return (0, N.jsx)(`div`, {
    className: `grid-dots h-full overflow-auto`,
    children: (0, N.jsxs)(`div`, {
      ref: t,
      className: `relative mx-auto my-4`,
      style: { width: ie, height: 448 },
      onPointerMove: (e) => i && a({ ...i, ...f(e) }),
      onPointerUp: () => a(null),
      onClick: (t) => t.target === t.currentTarget && e.onSelect(null),
      children: [
        (0, N.jsxs)(`svg`, {
          width: ie,
          height: 448,
          className: `pointer-events-none absolute inset-0`,
          children: [
            Array.from({ length: 9 }).map((e, t) =>
              (0, N.jsx)(
                `line`,
                {
                  x1: 160 + t * 116,
                  x2: 160 + t * 116,
                  y1: 8,
                  y2: 440,
                  stroke: `var(--color-border)`,
                  strokeOpacity: 0.35,
                  strokeDasharray: `2 6`,
                },
                t,
              ),
            ),
            [0, 1, 2, 3].map((t) => {
              let n = I(t),
                r = d(t),
                i = e.channels[t],
                a = r !== null && !i.mute;
              return (0, N.jsxs)(
                `g`,
                {
                  children: [
                    (0, N.jsx)(`line`, {
                      x1: re,
                      x2: P,
                      y1: n,
                      y2: n,
                      stroke: `var(--color-wire-dim)`,
                      strokeWidth: 2,
                    }),
                    r !== null &&
                      (0, N.jsxs)(N.Fragment, {
                        children: [
                          (0, N.jsx)(`line`, {
                            x1: r,
                            x2: P,
                            y1: n,
                            y2: n,
                            stroke: `var(--color-wire)`,
                            strokeOpacity: i.mute ? 0.3 : 0.9,
                            strokeWidth: 2,
                          }),
                          a &&
                            (0, N.jsx)(`line`, {
                              x1: r,
                              x2: P,
                              y1: n,
                              y2: n,
                              stroke: `var(--color-background)`,
                              strokeWidth: 2,
                              className: `wire-flow`,
                            }),
                        ],
                      }),
                    (0, N.jsx)(`circle`, {
                      cx: re,
                      cy: n,
                      r: 5,
                      fill: `var(--color-background)`,
                      stroke: i.input
                        ? `var(--color-wire)`
                        : `var(--color-wire-dim)`,
                      strokeWidth: 2,
                    }),
                    (0, N.jsx)(`circle`, {
                      cx: P,
                      cy: n,
                      r: 5,
                      fill: a ? `var(--color-wire)` : `var(--color-background)`,
                      stroke: a ? `var(--color-wire)` : `var(--color-wire-dim)`,
                      strokeWidth: 2,
                    }),
                  ],
                },
                t,
              );
            }),
            (0, N.jsx)(`line`, {
              x1: P,
              x2: P,
              y1: I(0),
              y2: I(3),
              stroke: `var(--color-wire-dim)`,
              strokeWidth: 2,
            }),
            (0, N.jsx)(`line`, {
              x1: P,
              x2: 1144,
              y1: 448 / 2,
              y2: 448 / 2,
              stroke: `var(--color-wire)`,
              strokeWidth: 2,
            }),
            (0, N.jsx)(`polygon`, {
              points: `1140,${448 / 2 - 5} 1148,${448 / 2} 1140,229`,
              fill: `var(--color-wire)`,
            }),
            e.links.map((e) => {
              let t = u(e.from),
                n = u(e.to);
              if (!t || !n) return null;
              let r = F(t.col) + 92 / 2 + 8,
                i = I(t.row),
                a = F(n.col) - 92 / 2,
                s = I(n.row),
                c = m(r, i, a, s),
                l = o === e.id;
              return (0, N.jsxs)(
                `g`,
                {
                  children: [
                    (0, N.jsx)(`path`, {
                      d: c,
                      stroke: `var(--color-background)`,
                      strokeWidth: 6,
                      fill: `none`,
                    }),
                    (0, N.jsx)(`path`, {
                      d: c,
                      stroke: `var(--color-wire)`,
                      strokeWidth: l ? 3 : 2,
                      fill: `none`,
                    }),
                    (0, N.jsx)(`path`, {
                      d: c,
                      stroke: `var(--color-background)`,
                      strokeWidth: 2,
                      fill: `none`,
                      className: `wire-flow`,
                    }),
                    (0, N.jsx)(`circle`, {
                      cx: r,
                      cy: i,
                      r: 4,
                      fill: `var(--color-wire)`,
                    }),
                  ],
                },
                e.id,
              );
            }),
            i &&
              (() => {
                let e = u(i.from);
                return (0, N.jsx)(`path`, {
                  d: m(F(e.col) + 92 / 2 + 8, I(e.row), i.x, i.y),
                  stroke: `var(--color-wire)`,
                  strokeWidth: 2,
                  strokeDasharray: `4 4`,
                  fill: `none`,
                });
              })(),
          ],
        }),
        (0, N.jsx)(`svg`, {
          width: ie,
          height: 448,
          className: `pointer-events-none absolute inset-0`,
          children: e.links.map((t) => {
            let n = u(t.from),
              r = u(t.to);
            if (!n || !r) return null;
            let i = m(
              F(n.col) + 92 / 2 + 8,
              I(n.row),
              F(r.col) - 92 / 2,
              I(r.row),
            );
            return (0, N.jsx)(
              `path`,
              {
                d: i,
                stroke: `transparent`,
                strokeWidth: 14,
                fill: `none`,
                className: `pointer-events-auto cursor-pointer`,
                onMouseEnter: () => s(t.id),
                onMouseLeave: () => s(null),
                onClick: () => e.onUnlink(t.id),
              },
              t.id,
            );
          }),
        }),
        e.channels.map((t, n) =>
          (0, N.jsxs)(
            `div`,
            {
              className: `absolute flex items-center gap-2`,
              style: { left: 0, top: I(n) - 18, height: 36, width: 116 },
              children: [
                (0, N.jsx)(`span`, {
                  className: `w-4 font-mono text-xs text-muted-foreground`,
                  children: n + 1,
                }),
                (0, N.jsx)(`button`, {
                  onClick: () => e.onChannel(n, { mute: !t.mute }),
                  className: `rounded p-1 ${t.mute ? `text-cat-drive` : `text-muted-foreground hover:text-foreground`}`,
                  title: `Mute`,
                  children: t.mute
                    ? (0, N.jsx)(E, { size: 14 })
                    : (0, N.jsx)(T, { size: 14 }),
                }),
                (0, N.jsx)(ae, {
                  value: t.trim,
                  onChange: (t) => e.onChannel(n, { trim: t }),
                }),
                (0, N.jsx)(`button`, {
                  onClick: () => e.onChannel(n, { input: !t.input }),
                  title: `Feed from main input`,
                  className: `rounded px-1 font-mono text-[9px] tracking-wider ${t.input ? `text-primary` : `text-muted-foreground/60 hover:text-foreground`}`,
                  children: `IN`,
                }),
              ],
            },
            n,
          ),
        ),
        [0, 1, 2, 3].map((t) =>
          Array.from({ length: 8 }).map((o, s) => {
            let l = c(t, s),
              d = n?.row === t && n.col === s,
              m = F(s) - 92 / 2,
              h = I(t) - 60 / 2;
            if (!l)
              return (0, N.jsx)(
                `div`,
                {
                  className: `absolute flex items-center justify-center`,
                  style: { left: m, top: h, width: 92, height: 60 },
                  onDragOver: (e) => {
                    (e.preventDefault(), r({ row: t, col: s }));
                  },
                  onDragLeave: () => r(null),
                  onDrop: (n) => {
                    (n.preventDefault(), r(null));
                    let i = n.dataTransfer.getData(`new`),
                      a = n.dataTransfer.getData(`move`);
                    i ? e.onDropNew(i, t, s) : a && e.onMove(a, t, s);
                  },
                  children: (0, N.jsx)(`div`, {
                    className: `flex items-center justify-center rounded-md border border-dashed transition-all ${d ? `h-full w-full border-primary bg-primary/10` : `size-6 border-transparent text-transparent hover:border-muted-foreground/40 hover:text-muted-foreground`}`,
                    children: (0, N.jsx)(ee, {
                      size: d ? 18 : 12,
                      className: d ? `text-primary` : ``,
                    }),
                  }),
                },
                `${t}-${s}`,
              );
            let g = j(l.type),
              _ = g.icon,
              v = M[g.cat],
              y = e.selected === l.id,
              b = i && i.from !== l.id && p(u(i.from), l);
            return (0, N.jsxs)(
              `div`,
              {
                draggable: !0,
                onDragStart: (e) => e.dataTransfer.setData(`move`, l.id),
                onClick: () => e.onSelect(l.id),
                onPointerUp: () => {
                  i && b && e.onLink(i.from, l.id);
                },
                className: `group absolute flex cursor-grab flex-col items-center justify-center gap-1 rounded-md border bg-panel-2 transition-all active:cursor-grabbing ${y ? `ring-2 ring-offset-2 ring-offset-background` : `hover:brightness-125`} ${l.bypass ? `opacity-40` : ``} ${b ? `ring-2 ring-primary/70` : ``}`,
                style: {
                  left: m,
                  top: h,
                  width: 92,
                  height: 60,
                  borderColor: `color-mix(in oklab, ${v} 45%, var(--color-border))`,
                  "--tw-ring-color": v,
                  backgroundImage: `linear-gradient(180deg, color-mix(in oklab, ${v} 18%, transparent), transparent)`,
                },
                children: [
                  (0, N.jsx)(`span`, {
                    className: `absolute left-0 top-2 bottom-2 w-[3px] rounded-r`,
                    style: { background: v },
                  }),
                  (0, N.jsx)(_, { size: 18, style: { color: v } }),
                  (0, N.jsx)(`span`, {
                    className: `text-[11px] font-medium leading-none`,
                    children: g.label,
                  }),
                  (0, N.jsx)(`span`, {
                    title: `Drag to another channel to split`,
                    onPointerDown: (e) => {
                      (e.stopPropagation(),
                        e.preventDefault(),
                        a({ from: l.id, ...f(e) }));
                    },
                    onDragStart: (e) => e.preventDefault(),
                    className: `absolute -right-[7px] top-1/2 size-3 -translate-y-1/2 cursor-crosshair rounded-full border-2 border-background bg-muted-foreground opacity-0 transition group-hover:opacity-100 hover:scale-150 hover:bg-primary`,
                  }),
                ],
              },
              l.id,
            );
          }),
        ),
        o &&
          (0, N.jsxs)(`div`, {
            className: `pointer-events-none absolute right-2 top-0 flex items-center gap-1 rounded bg-panel-2 px-2 py-1 text-[10px] text-muted-foreground`,
            children: [(0, N.jsx)(ne, { size: 10 }), ` click cable to remove`],
          }),
      ],
    }),
  });
}
function ae({ value: e, onChange: t }) {
  let n = (0, l.useRef)(null),
    r = (e + 24) / 36;
  return (0, N.jsxs)(`div`, {
    className: `group flex cursor-ns-resize select-none flex-col gap-1`,
    title: `Input trim — drag up/down, double‑click to reset`,
    onPointerDown: (t) => {
      (t.target.setPointerCapture(t.pointerId),
        (n.current = { y: t.clientY, v: e }));
    },
    onPointerMove: (e) =>
      n.current &&
      t(
        Math.max(
          -24,
          Math.min(
            12,
            Math.round((n.current.v + (n.current.y - e.clientY) / 4) * 2) / 2,
          ),
        ),
      ),
    onPointerUp: () => (n.current = null),
    onDoubleClick: () => t(0),
    children: [
      (0, N.jsxs)(`span`, {
        className: `font-mono text-[10px] tabular-nums text-muted-foreground group-hover:text-foreground`,
        children: [e > 0 ? `+` : ``, e.toFixed(1), ` dB`],
      }),
      (0, N.jsxs)(`span`, {
        className: `relative h-[3px] w-12 overflow-hidden rounded-full bg-muted`,
        children: [
          (0, N.jsx)(`span`, {
            className: `absolute inset-y-0 left-0 rounded-full bg-muted-foreground group-hover:bg-primary`,
            style: { width: `${r * 100}%` },
          }),
          (0, N.jsx)(`span`, {
            className: `absolute inset-y-0 w-px bg-foreground/50`,
            style: { left: `${(24 / 36) * 100}%` },
          }),
        ],
      }),
    ],
  });
}
function oe(e) {
  var t,
    n,
    r = ``;
  if (typeof e == `string` || typeof e == `number`) r += e;
  else if (typeof e == `object`)
    if (Array.isArray(e)) {
      var i = e.length;
      for (t = 0; t < i; t++)
        e[t] && (n = oe(e[t])) && (r && (r += ` `), (r += n));
    } else for (n in e) e[n] && (r && (r += ` `), (r += n));
  return r;
}
function R() {
  for (var e, t, n = 0, r = ``, i = arguments.length; n < i; n++)
    (e = arguments[n]) && (t = oe(e)) && (r && (r += ` `), (r += t));
  return r;
}
// ... tailwind-merge runtime omitted ...
function xt(...e) {
  return bt(R(e));
}
var St = -135,
  Ct = 270;
function wt({
  value: e,
  min: t,
  max: n,
  onChange: r,
  size: i = 52,
  label: a,
  unit: o = ``,
  accent: s = `var(--color-primary)`,
  labelSide: c = `bottom`,
  showValue: u = !0,
}) {
  let d = (0, l.useId)().replace(/:/g, ``),
    f = (0, l.useRef)(null),
    [p, m] = (0, l.useState)(!1),
    h = Math.min(1, Math.max(0, (e - t) / (n - t))),
    g = St + h * Ct,
    _ = Math.max(2.5, i * 0.07),
    v = Math.max(3, i * 0.085),
    y = i / 2,
    b = y - _ / 2 - v - 2,
    x = Math.max(5, b - _ / 2 - Math.max(3, i * 0.06)),
    ee = Math.max(1.5, i * 0.045),
    S = (e, t) => [
      y + t * Math.sin((e * Math.PI) / 180),
      y - t * Math.cos((e * Math.PI) / 180),
    ],
    C = (e, t) => {
      let [n, r] = S(e, b),
        [i, a] = S(t, b);
      return `M${n} ${r} A${b} ${b} 0 ${+(t - e > 180)} 1 ${i} ${a}`;
    },
    w = t < 0 && n > 0,
    te = n - t >= 100 ? 0 : n - t >= 20 ? 1 : 2,
    T = `${w && e > 0 ? `+` : ``}${e.toFixed(te)}`,
    E = (e) => r(Math.min(n, Math.max(t, e))),
    D = (n - t) / 100,
    ne = [0, 0.25, 0.5, 0.75, 1].map((e) => {
      let t = St + e * Ct,
        [n, r] = S(t, b + _ / 2 + 1),
        [i, a] = S(t, b + _ / 2 + 1 + v * (e === 0.5 ? 1.1 : 1));
      return (0, N.jsx)(
        `line`,
        {
          x1: n,
          y1: r,
          x2: i,
          y2: a,
          stroke: `var(--color-muted-foreground)`,
          strokeOpacity: e === 0.5 ? 0.7 : 0.35,
          strokeWidth: 1.25,
          strokeLinecap: `round`,
        },
        e,
      );
    }),
    O = (0, N.jsxs)(`div`, {
      className: xt(
        `flex flex-col gap-0.5 leading-none`,
        c === `bottom`
          ? `items-center`
          : c === `left`
            ? `items-end text-right`
            : `items-start text-left`,
        c !== `bottom` && `w-[62px]`,
      ),
      children: [
        a &&
          (0, N.jsx)(`div`, {
            className: `text-[10px] font-medium uppercase tracking-widest text-muted-foreground`,
            children: a,
          }),
        u &&
          (0, N.jsxs)(`div`, {
            className: xt(
              `font-mono tabular-nums text-foreground`,
              c === `bottom` ? `text-[11px]` : `text-[13px] font-medium`,
            ),
            children: [
              T,
              o &&
                (0, N.jsxs)(`span`, {
                  className: xt(
                    `text-muted-foreground`,
                    c !== `bottom` && `text-[10px] font-normal`,
                  ),
                  children: [` `, o],
                }),
            ],
          }),
      ],
    });
  return (0, N.jsxs)(`div`, {
    className: xt(
      `flex select-none`,
      c === `bottom`
        ? `flex-col items-center gap-1.5`
        : `flex-row items-center gap-3`,
      c === `left` && `flex-row-reverse`,
    ),
    children: [
      (0, N.jsxs)(`svg`, {
        width: i,
        height: i,
        viewBox: `0 0 ${i} ${i}`,
        role: `slider`,
        "aria-label": a,
        "aria-valuemin": t,
        "aria-valuemax": n,
        "aria-valuenow": e,
        tabIndex: 0,
        className: `cursor-ns-resize rounded-full outline-none focus-visible:ring-2 focus-visible:ring-primary/60`,
        style: { touchAction: `none` },
        onPointerDown: (t) => {
          (t.target.setPointerCapture(t.pointerId),
            (f.current = { y: t.clientY, v: e }),
            m(!0));
        },
        onPointerMove: (e) => {
          f.current &&
            E(
              f.current.v +
                ((f.current.y - e.clientY) / (e.shiftKey ? 600 : 150)) *
                  (n - t),
            );
        },
        onPointerUp: () => {
          ((f.current = null), m(!1));
        },
        onPointerCancel: () => {
          ((f.current = null), m(!1));
        },
        onDoubleClick: () => r(w ? 0 : t),
        onKeyDown: (r) => {
          let i = r.key;
          i === `ArrowUp` || i === `ArrowRight`
            ? (r.preventDefault(), E(e + D))
            : i === `ArrowDown` || i === `ArrowLeft`
              ? (r.preventDefault(), E(e - D))
              : i === `PageUp`
                ? (r.preventDefault(), E(e + D * 10))
                : i === `PageDown`
                  ? (r.preventDefault(), E(e - D * 10))
                  : i === `Home`
                    ? (r.preventDefault(), E(t))
                    : i === `End` && (r.preventDefault(), E(n));
        },
        children: [
          (0, N.jsx)(`defs`, {
            children: (0, N.jsxs)(`radialGradient`, {
              id: `body-${d}`,
              cx: `50%`,
              cy: `34%`,
              r: `78%`,
              children: [
                (0, N.jsx)(`stop`, {
                  offset: `0%`,
                  stopColor: `color-mix(in oklab, var(--color-panel-2) 62%, white)`,
                }),
                (0, N.jsx)(`stop`, {
                  offset: `100%`,
                  stopColor: `var(--color-panel-2)`,
                }),
              ],
            }),
          }),
          ne,
          (0, N.jsx)(`path`, {
            d: C(St, 135),
            stroke: `var(--color-muted)`,
            strokeWidth: _,
            fill: `none`,
            strokeLinecap: `round`,
          }),
          h > 0.004 &&
            (0, N.jsx)(`path`, {
              d: C(St, g),
              stroke: s,
              strokeWidth: _,
              fill: `none`,
              strokeLinecap: `round`,
              opacity: p ? 1 : 0.92,
            }),
          (0, N.jsx)(`circle`, {
            cx: y,
            cy: y,
            r: x,
            fill: `url(#body-${d})`,
            stroke: p ? s : `var(--color-border)`,
            strokeWidth: p ? 1.5 : 1,
          }),
          (0, N.jsx)(`line`, {
            x1: y,
            y1: y,
            x2: S(g, x * 0.9)[0],
            y2: S(g, x * 0.9)[1],
            stroke: p ? s : `var(--color-foreground)`,
            strokeWidth: ee,
            strokeLinecap: `round`,
          }),
        ],
      }),
      c !== `bottom` && O,
      c === `bottom` &&
        (0, N.jsxs)(N.Fragment, {
          children: [
            a &&
              (0, N.jsx)(`div`, {
                className: `text-[10px] font-medium uppercase tracking-widest text-muted-foreground`,
                children: a,
              }),
            u &&
              (0, N.jsxs)(`div`, {
                className: `font-mono text-[11px] tabular-nums text-foreground`,
                children: [
                  T,
                  o &&
                    (0, N.jsxs)(`span`, {
                      className: `text-muted-foreground`,
                      children: [` `, o],
                    }),
                ],
              }),
          ],
        }),
    ],
  });
}
var Tt = 100,
  $ = (e, t, n, r = `b${++Tt}`) => ({
    id: r,
    type: e,
    row: t,
    col: n,
    bypass: !1,
    values: j(e).params.map((e) => e.v),
  });
function Et() {
  let [e, t] = (0, l.useState)(() => [
      $(`gate`, 0, 0, `b1`),
      $(`od`, 0, 1, `b2`),
      $(`amp`, 0, 2, `b3`),
      $(`cab`, 0, 3, `b4`),
      $(`delay`, 0, 5, `b5`),
      $(`reverb`, 0, 6, `b6`),
      $(`pdelay`, 1, 4, `b7`),
      $(`chorus`, 1, 5, `b8`),
    ]),
    [n, r] = (0, l.useState)(() => [{ id: `l1`, from: `b4`, to: `b7` }]),
    [i, a] = (0, l.useState)([
      { input: !0, mute: !1, trim: 0 },
      { input: !1, mute: !1, trim: -3 },
      { input: !1, mute: !1, trim: 0 },
      { input: !1, mute: !1, trim: 0 },
    ]),
    [o, s] = (0, l.useState)(`b3`),
    [c, u] = (0, l.useState)(`E`),
    [d, f] = (0, l.useState)(0),
    [p, m] = (0, l.useState)(0),
    _ = e.find((e) => e.id === o),
    v = (e) => {
      (t((t) => t.filter((t) => t.id !== e)),
        r((t) => t.filter((t) => t.from !== e && t.to !== e)),
        s(null));
    };
  (0, l.useEffect)(() => {
    let e = (e) => {
      (e.key === `Delete` || e.key === `Backspace`) &&
        o &&
        !(e.target instanceof HTMLInputElement) &&
        v(o);
    };
    return (
      window.addEventListener(`keydown`, e),
      () => window.removeEventListener(`keydown`, e)
    );
  });
  let y = (e, n) => t((t) => t.map((t) => (t.id === e ? n(t) : t)));
  return (0, N.jsxs)(`div`, {
    className: `flex h-screen flex-col overflow-hidden bg-background text-foreground`,
    children: [
      (0, N.jsxs)(`header`, {
        className: `flex h-20 shrink-0 items-center gap-6 border-b bg-panel px-5`,
        children: [
          (0, N.jsx)(wt, {
            value: d,
            min: -24,
            max: 24,
            onChange: f,
            size: 48,
            label: `Input`,
            unit: `dB`,
            labelSide: `right`,
          }),
          (0, N.jsx)(Dt, {}),
          (0, N.jsxs)(`div`, {
            className: `flex items-center gap-1`,
            children: [
              (0, N.jsx)(Ot, { children: (0, N.jsx)(h, { size: 14 }) }),
              (0, N.jsxs)(`div`, {
                className: `w-32 px-2`,
                children: [
                  (0, N.jsx)(`div`, {
                    className: `text-sm font-semibold`,
                    children: `04-rainbow`,
                  }),
                  (0, N.jsx)(`div`, {
                    className: `text-[11px] text-muted-foreground`,
                    children: `Silence`,
                  }),
                ],
              }),
              (0, N.jsx)(Ot, { children: (0, N.jsx)(g, { size: 14 }) }),
              (0, N.jsx)(Ot, { children: (0, N.jsx)(b, { size: 14 }) }),
            ],
          }),
          (0, N.jsx)(`div`, {
            className: `flex gap-1.5`,
            children: `ABCDEFGH`
              .split(``)
              .map((e) =>
                (0, N.jsx)(
                  `button`,
                  {
                    onClick: () => u(e),
                    className: `size-8 rounded font-mono text-xs ${c === e ? `bg-primary text-primary-foreground` : `bg-muted text-muted-foreground hover:text-foreground`}`,
                    children: e,
                  },
                  e,
                ),
              ),
          }),
          (0, N.jsx)(`div`, { className: `flex-1` }),
          (0, N.jsxs)(`button`, {
            className: `flex items-center gap-2 rounded border px-3 py-2 text-xs tracking-widest text-muted-foreground hover:text-foreground`,
            children: [(0, N.jsx)(x, { size: 14 }), ` TUNER`],
          }),
          (0, N.jsxs)(`div`, {
            className: `flex gap-4 font-mono text-[11px] text-muted-foreground`,
            children: [
              (0, N.jsxs)(`span`, {
                children: [
                  `DSP `,
                  (0, N.jsx)(`span`, {
                    className: `text-foreground`,
                    children: `4%`,
                  }),
                ],
              }),
              (0, N.jsxs)(`span`, {
                children: [
                  `LAT `,
                  (0, N.jsx)(`span`, {
                    className: `text-primary`,
                    children: `5.33 ms`,
                  }),
                ],
              }),
            ],
          }),
          (0, N.jsx)(Dt, {}),
          (0, N.jsx)(wt, {
            value: p,
            min: -60,
            max: 6,
            onChange: m,
            size: 48,
            label: `Master`,
            unit: `dB`,
            labelSide: `left`,
          }),
        ],
      }),
      (0, N.jsxs)(`div`, {
        className: `flex min-h-0 flex-1`,
        children: [
          (0, N.jsx)(`aside`, {
            className: `w-52 shrink-0 overflow-y-auto border-r bg-panel px-3 py-3`,
            children: A.map((e) =>
              (0, N.jsxs)(
                `div`,
                {
                  className: `mb-3`,
                  children: [
                    (0, N.jsx)(`div`, {
                      className: `mb-1 px-1 text-[10px] font-semibold uppercase tracking-widest`,
                      style: { color: M[e.id] },
                      children: e.label,
                    }),
                    k
                      .filter((t) => t.cat === e.id)
                      .map((e) =>
                        (0, N.jsxs)(
                          `div`,
                          {
                            draggable: !0,
                            onDragStart: (t) =>
                              t.dataTransfer.setData(`new`, e.type),
                            className: `flex cursor-grab items-center gap-2.5 rounded px-2 py-1.5 text-[13px] text-foreground/85 hover:bg-muted`,
                            children: [
                              (0, N.jsx)(e.icon, {
                                size: 14,
                                style: { color: M[e.cat] },
                              }),
                              ` `,
                              e.label,
                            ],
                          },
                          e.type,
                        ),
                      ),
                  ],
                },
                e.id,
              ),
            ),
          }),
          (0, N.jsx)(`main`, {
            className: `min-w-0 flex-1`,
            children: (0, N.jsx)(L, {
              blocks: e,
              links: n,
              channels: i,
              selected: o,
              onSelect: s,
              onDropNew: (e, n, r) => {
                let i = $(e, n, r);
                (t((e) => [...e, i]), s(i.id));
              },
              onMove: (t, n, i) => {
                (y(t, (e) => ({ ...e, row: n, col: i })),
                  r((r) =>
                    r.filter((r) => {
                      let a = e.find((e) => e.id === r.from),
                        o = e.find((e) => e.id === r.to),
                        s = a.id === t ? { row: n, col: i } : a,
                        c = o.id === t ? { row: n, col: i } : o;
                      return s.row !== c.row && c.col > s.col;
                    }),
                  ));
              },
              onLink: (e, t) =>
                r((n) => [...n, { id: `l${++Tt}`, from: e, to: t }]),
              onUnlink: (e) => r((t) => t.filter((t) => t.id !== e)),
              onChannel: (e, t) =>
                a((n) => n.map((n, r) => (r === e ? { ...n, ...t } : n))),
            }),
          }),
        ],
      }),
      (0, N.jsx)(`footer`, {
        className: `flex h-40 shrink-0 items-center gap-10 border-t bg-panel px-6`,
        children: _
          ? (() => {
              let e = j(_.type);
              return (0, N.jsxs)(N.Fragment, {
                children: [
                  (0, N.jsxs)(`div`, {
                    className: `w-40`,
                    children: [
                      (0, N.jsxs)(`div`, {
                        className: `flex items-center gap-2 text-base font-semibold`,
                        children: [
                          (0, N.jsx)(e.icon, {
                            size: 16,
                            style: { color: M[e.cat] },
                          }),
                          e.label,
                        ],
                      }),
                      (0, N.jsxs)(`div`, {
                        className: `mt-1 text-[11px] text-muted-foreground`,
                        children: [
                          `Channel `,
                          _.row + 1,
                          ` · Slot `,
                          _.col + 1,
                        ],
                      }),
                      (0, N.jsxs)(`div`, {
                        className: `mt-3 flex gap-1.5`,
                        children: [
                          (0, N.jsx)(kt, {
                            active: _.bypass,
                            onClick: () =>
                              y(_.id, (e) => ({ ...e, bypass: !e.bypass })),
                            children: `Bypass`,
                          }),
                          (0, N.jsx)(kt, {
                            onClick: () =>
                              y(_.id, (t) => ({
                                ...t,
                                values: e.params.map((e) => e.v),
                              })),
                            children: `Reset`,
                          }),
                          (0, N.jsx)(kt, {
                            onClick: () => v(_.id),
                            children: `Delete`,
                          }),
                        ],
                      }),
                    ],
                  }),
                  (0, N.jsx)(`div`, {
                    className: `flex flex-1 justify-center gap-8`,
                    children: e.params.map((t, n) =>
                      (0, N.jsx)(
                        wt,
                        {
                          label: t.k,
                          unit: t.unit ?? ``,
                          min: t.min,
                          max: t.max,
                          value: _.values[n] ?? t.v,
                          accent: M[e.cat],
                          onChange: (e) =>
                            y(_.id, (t) => ({
                              ...t,
                              values: t.values.map((t, r) => (r === n ? e : t)),
                            })),
                        },
                        t.k,
                      ),
                    ),
                  }),
                ],
              });
            })()
          : (0, N.jsx)(`div`, {
              className: `w-full text-center text-sm text-muted-foreground`,
              children: `Select a block to edit · drag from the tray to add · drag a block's right edge to another channel to split`,
            }),
      }),
    ],
  });
}
var Dt = () => (0, N.jsx)(`div`, { className: `h-10 w-px shrink-0 bg-border` }),
  Ot = ({ children: e }) =>
    (0, N.jsx)(`button`, {
      className: `flex size-7 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground`,
      children: e,
    }),
  kt = ({ children: e, onClick: t, active: n }) =>
    (0, N.jsx)(`button`, {
      onClick: t,
      className: `rounded border px-2 py-1 text-[11px] ${n ? `border-primary bg-primary/15 text-primary` : `text-muted-foreground hover:text-foreground`}`,
      children: e,
    });
export { Et as component };
