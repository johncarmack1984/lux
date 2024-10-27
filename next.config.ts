import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  reactStrictMode: true,
  output: "export",
  distDir: "./dist",
  images: {
    unoptimized: true,
  },
};

export default nextConfig;
