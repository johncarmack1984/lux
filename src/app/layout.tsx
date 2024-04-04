import { Toaster } from "@/components/ui/sonner";
import type { Metadata } from "next";
import { Inter } from "next/font/google";
import "./globals.css";

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
        <main className="flex min-h-screen flex-col items-center justify-start py-[7.5%] sm:px-12">
          {children}
        </main>
        <Toaster closeButton={true} />
      </body>
    </html>
  );
}
