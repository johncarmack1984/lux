/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  swcMinify: true,
  output: "export",
  distDir: "./dist",
  images: {
    unoptimized: true,
  },
};

export default nextConfig;
