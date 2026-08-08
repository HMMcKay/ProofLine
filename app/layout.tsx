import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  metadataBase: new URL("https://proofline-verifiable-video.mckayxj.chatgpt.site"),
  title: {
    default: "ProofLine — Public video provenance",
    template: "%s · ProofLine",
  },
  description: "Device-signed, server-receipted public field video with inspectable completeness and provenance evidence.",
  icons: {
    icon: "/favicon.svg",
    shortcut: "/favicon.svg",
  },
  openGraph: {
    title: "ProofLine — Every received frame leaves a receipt",
    description: "Public field video with signed fragment chains, server receipt times, sensor context, and honest completeness reports.",
    type: "website",
    images: [{ url: "/og.png", width: 1730, height: 909, alt: "Abstract signed video fragments flowing into an append-only evidence vault" }],
  },
  twitter: { card: "summary_large_image", images: ["/og.png"] },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className="dark">
      <body
        className={`${geistSans.variable} ${geistMono.variable} antialiased`}
      >
        <a className="skip-link" href="#main-content">Skip to content</a>
        {children}
      </body>
    </html>
  );
}
