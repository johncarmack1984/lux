import { Toaster } from "@/components/ui/sonner";
import type { Metadata } from "next";
import { Inter } from "next/font/google";
import "./globals.css";
import ColorPicker from "@/components/color-picker/picker";

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
        <ColorPicker className="absolute top-3 right-5" />
        <main className="flex min-h-screen flex-col items-center justify-start py-[7.5%] sm:px-12">
          {children}
        </main>
        <Toaster closeButton={true} />
      </body>
    </html>
  );
}
