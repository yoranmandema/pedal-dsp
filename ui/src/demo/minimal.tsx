import { render } from "preact";
import { Graph } from "../components/Graph";
import "../tailwind.css";

// Minimal render-only test: proves <Graph> paints with the real theme.
function Minimal() {
  return (
    <div class="p-4">
      <Graph
        axes={{
          x: { type: "log", min: 20, max: 20000, units: "Hz", ticks: 6 },
          y: { type: "linear", min: -12, max: 12, units: "dB", ticks: 5 },
        }}
        dataDomain={[20, 20000]}
        series={[
          {
            key: "t",
            label: "Test",
            color: "#e8a33d",
            kind: "line",
            data: Array.from({ length: 120 }, (_, i) => {
              const t = i / 119;
              const db = 6 * Math.sin(t * Math.PI * 2) + 2 * Math.sin(t * Math.PI * 7);
              return Math.round(db * 10) / 10;
            }),
          },
        ]}
      />
    </div>
  );
}

const root = document.getElementById("demo");
if (root) render(<Minimal />, root);
