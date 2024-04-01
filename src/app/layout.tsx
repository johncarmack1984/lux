import type { Metadata } from "next";
import { Inter } from "next/font/google";
import "./globals.css";
import Providers from "./providers";

const inter = Inter({ subsets: ["latin"] });

export const metadata: Metadata = {
  title: "Lux.app",
  description: "Built with Tauri and Next.js",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en">
      <body className={`${inter.className} dark`}>
        <Providers>
          <main className="flex min-h-screen flex-col items-center justify-start py-[7.5%] sm:px-12">
            {children}
          </main>
        </Providers>
      </body>
    </html>
  );
}
