// Deterministic, offline SVG figures for the Happy Science first-install examples.
// They visualize bundled demonstration data and never fetch external assets.

const COLORS = ["#0f857b", "#55cbb9", "#e1515d", "#d6a63a", "#5b7fa3"];
const INK = "#18312f";
const MUTED = "#657875";

function svgUri(width: number, height: number, body: string): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" font-family="Segoe UI, Arial, sans-serif"><rect width="${width}" height="${height}" fill="#ffffff"/>${body}</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

function barChart(title: string, bars: { label: string; value: number }[]): string {
  const width = 560;
  const height = 320;
  const left = 48;
  const right = 20;
  const top = 48;
  const bottom = 62;
  const plotWidth = width - left - right;
  const plotHeight = height - top - bottom;
  const baseline = height - bottom;
  const max = Math.max(...bars.map((bar) => bar.value));
  const slot = plotWidth / bars.length;
  const barWidth = slot * 0.5;
  const grid = [0, 0.25, 0.5, 0.75, 1]
    .map((fraction) => {
      const y = baseline - fraction * plotHeight;
      return `<line x1="${left}" y1="${y}" x2="${width - right}" y2="${y}" stroke="#e7efed"/><text x="${left - 8}" y="${y + 3}" font-size="10" fill="${MUTED}" text-anchor="end">${Math.round(fraction * max)}</text>`;
    })
    .join("");
  const columns = bars
    .map((bar, index) => {
      const columnHeight = (bar.value / max) * plotHeight;
      const x = left + index * slot + (slot - barWidth) / 2;
      const y = baseline - columnHeight;
      return `<rect x="${x}" y="${y}" width="${barWidth}" height="${columnHeight}" rx="4" fill="${COLORS[index % COLORS.length]}"/><text x="${x + barWidth / 2}" y="${y - 7}" font-size="11" fill="${INK}" text-anchor="middle" font-weight="600">${bar.value}</text><text x="${x + barWidth / 2}" y="${baseline + 18}" font-size="10" fill="${MUTED}" text-anchor="middle">${bar.label}</text>`;
    })
    .join("");
  return svgUri(
    width,
    height,
    `<text x="${left}" y="28" font-size="15" font-weight="700" fill="${INK}">${title}</text>${grid}<line x1="${left}" y1="${baseline}" x2="${width - right}" y2="${baseline}" stroke="#b8c8c5"/>${columns}`,
  );
}

function lineChart(
  title: string,
  yLabel: string,
  xLabel: string,
  points: number[],
  options: { yMin: number; yMax: number; reference: { value: number; label: string } },
): string {
  const width = 560;
  const height = 320;
  const left = 58;
  const right = 24;
  const top = 48;
  const bottom = 52;
  const plotWidth = width - left - right;
  const plotHeight = height - top - bottom;
  const baseline = height - bottom;
  const x = (index: number) => left + (index / (points.length - 1)) * plotWidth;
  const y = (value: number) =>
    baseline - ((value - options.yMin) / (options.yMax - options.yMin)) * plotHeight;
  const grid = [0, 0.25, 0.5, 0.75, 1]
    .map((fraction) => {
      const value = options.yMin + fraction * (options.yMax - options.yMin);
      const position = baseline - fraction * plotHeight;
      return `<line x1="${left}" y1="${position}" x2="${width - right}" y2="${position}" stroke="#e7efed"/><text x="${left - 8}" y="${position + 3}" font-size="10" fill="${MUTED}" text-anchor="end">${value.toFixed(2)}</text>`;
    })
    .join("");
  const path = points
    .map((value, index) => `${index === 0 ? "M" : "L"}${x(index).toFixed(1)},${y(value).toFixed(1)}`)
    .join(" ");
  const dots = points
    .map(
      (value, index) =>
        `<circle cx="${x(index).toFixed(1)}" cy="${y(value).toFixed(1)}" r="3" fill="#0f857b"/>`,
    )
    .join("");
  const referenceY = y(options.reference.value);
  return svgUri(
    width,
    height,
    `<text x="${left}" y="28" font-size="15" font-weight="700" fill="${INK}">${title}</text><text x="15" y="${top + plotHeight / 2}" font-size="10" fill="${MUTED}" transform="rotate(-90 15 ${top + plotHeight / 2})" text-anchor="middle">${yLabel}</text>${grid}<line x1="${left}" y1="${baseline}" x2="${width - right}" y2="${baseline}" stroke="#b8c8c5"/><line x1="${left}" y1="${referenceY}" x2="${width - right}" y2="${referenceY}" stroke="#e1515d" stroke-width="1.5" stroke-dasharray="6 4"/><text x="${width - right}" y="${referenceY - 7}" font-size="10" fill="#e1515d" text-anchor="end">${options.reference.label}</text><path d="${path}" fill="none" stroke="#0f857b" stroke-width="2.5"/>${dots}<text x="${width - right}" y="${baseline + 20}" font-size="10" fill="${MUTED}" text-anchor="end">${xLabel}</text>`,
  );
}

export const evidenceBalanceFigure = barChart("Evidence map · direction of findings", [
  { label: "Supports", value: 7 },
  { label: "Qualifies", value: 4 },
  { label: "Contradicts", value: 2 },
  { label: "Unclear", value: 1 },
]);

export const reproductionFigure = lineChart(
  "Dose-response reproduction · estimate convergence",
  "effect estimate",
  "bootstrap batch",
  [-0.31, -0.36, -0.39, -0.405, -0.412, -0.415, -0.416],
  { yMin: -0.46, yMax: -0.24, reference: { value: -0.42, label: "reference -0.420" } },
);

export const claimAuditFigure = barChart("Manuscript claim audit", [
  { label: "Traced", value: 18 },
  { label: "Unlinked", value: 3 },
  { label: "Mismatch", value: 1 },
]);
