import { colord } from "colord";
import type { RgbaColor } from "react-colorful";

const defaultBuffer = [0, 0, 0, 0, 0, 0];

const defaultRgba = { r: 0, g: 0, b: 0, a: 0 };

const bufferToLuminance = (buffer: number[] | null) => {
  if (!buffer) return 0;
  if (!buffer[5]) return 0;
  return buffer[5] / 255;
};

const alphaToBrightness = (a: number) => Math.round(a * 255);

const bufferToRgba = (buffer?: number[]) => {
  if (!buffer) return { r: 0, g: 0, b: 0, a: 1 };

  const a = bufferToLuminance(buffer);

  return { r: buffer[0], g: buffer[1], b: buffer[2], a };
};

const rgbaToBuffer = (color: RgbaColor) => {
  const { r, g, b, a } = color;
  return [r, g, b, 0, 0, alphaToBrightness(a)];
};

const hexToBuffer = (hex: string, a: number) => {
  const { r, g, b } = colord(hex).toRgb();
  return [r, g, b, 0, 0, a];
};

export {
  bufferToLuminance,
  bufferToRgba,
  rgbaToBuffer,
  hexToBuffer,
  defaultBuffer,
  defaultRgba,
};
