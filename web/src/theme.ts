function rgb(color: string) {
  return [1, 3, 5].map((i) => parseInt(color.slice(i, i + 2), 16) / 255);
}
export function luminance(color: string) {
  const [r, g, b] = rgb(color).map((v) =>
    v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4,
  );
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
export function contrast(a: string, b: string) {
  const l = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (l[0] + 0.05) / (l[1] + 0.05);
}
export function readableAccent(value: string) {
  let color = /^#[0-9a-f]{6}$/i.test(value) ? value : "#78cfbf";
  for (let i = 0; i < 16 && contrast(color, "#11171b") < 4.5; i++) {
    const c = rgb(color).map((v) => Math.round((v + (1 - v) * 0.15) * 255));
    color = "#" + c.map((v) => v.toString(16).padStart(2, "0")).join("");
  }
  return color;
}
export function applyAccent(value: string) {
  const c = readableAccent(value);
  document.documentElement.style.setProperty("--accent", c);
  document.documentElement.style.setProperty(
    "--accent-rgb",
    rgb(c)
      .map((v) => Math.round(v * 255))
      .join(","),
  );
}
