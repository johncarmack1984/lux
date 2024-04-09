function kelvinToRgb(kelvin) {
  let temp = kelvin / 100;
  let red, green, blue;

  if (temp <= 66) {
    red = 255;
    green = 99.4708025861 * Math.log(temp) - 161.1195681661;
    if (temp <= 19) {
      blue = 0;
    } else {
      blue = 138.5177312231 * Math.log(temp - 10) - 305.0447927307;
    }
  } else {
    red = 329.698727446 * Math.pow(temp - 60, -0.1332047592);
    green = 288.1221695283 * Math.pow(temp - 60, -0.0755148492);
    blue = 255;
  }

  return [clamp(red, 0, 255), clamp(green, 0, 255), clamp(blue, 0, 255)];
}

function rgbawToRgb(rgbaw) {
  let amberRgb = kelvinToRgb(2700);
  let daylightRgb = kelvinToRgb(6500);

  let r = rgbaw[0] + amberRgb[0] * rgbaw[3] + daylightRgb[0] * rgbaw[4];
  let g = rgbaw[1] + amberRgb[1] * rgbaw[3] + daylightRgb[1] * rgbaw[4];
  let b = rgbaw[2] + amberRgb[2] * rgbaw[3] + daylightRgb[2] * rgbaw[4];

  return [r, g, b];
}

function applyBrightness(rgb, brightness) {
  return rgb.map((component) => Math.round(component * brightness));
}

function rgbToHsl(rgb) {
  let r = rgb[0] / 255,
    g = rgb[1] / 255,
    b = rgb[2] / 255;

  let max = Math.max(r, g, b),
    min = Math.min(r, g, b),
    h,
    s,
    l = (max + min) / 2;

  if (max === min) {
    h = s = 0;
  } else {
    let d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case r:
        h = (g - b) / d + (g < b ? 6 : 0);
        break;
      case g:
        h = (b - r) / d + 2;
        break;
      case b:
        h = (r - g) / d + 4;
        break;
    }
    h /= 6;
  }

  return [Math.round(h * 360), Math.round(s * 100), Math.round(l * 100)];
}

function clamp(x, min, max) {
  return Math.min(Math.max(x, min), max);
}

// Example usage:
let rgbaw = [0, 0, 0, 0.5, 0.5, 0.8]; // RGBAW + Brightness
let effectiveRgb = rgbawToRgb(rgbaw);
let adjustedRgb = applyBrightness(effectiveRgb, rgbaw[5]);
let hsl = rgbToHsl(adjustedRgb);

console.log("HSL:", hsl);
