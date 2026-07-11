#!/usr/bin/env swift
//
// gen-ios-icons.swift — reproducible, dependency-free iOS app-icon generator (Story 15.1)
//
// Draws an OPAQUE keeper-green (#0F6E5C) full-bleed square icon with a simple, legible
// centered white "keep"/messenger mark (a rounded speech bubble with a small tail), and
// writes the 18 exact AppIcon filenames at their exact pixel sizes into the committed
// asset catalog. The vector mark is drawn fresh at every target size (never upscaled from a
// small raster) so it stays crisp at 20px and at 1024px alike.
//
// Requirements: macOS Swift toolchain (CoreGraphics + ImageIO). No third-party deps, no
// network. Run from the repo root:
//
//     swift scripts/gen-ios-icons.swift
//
// Output PNGs are opaque (no alpha channel): we render into an RGB (no-alpha) bitmap
// context, so the encoded PNG has no alpha. Every written file is re-read and checked for
// exact pixel size + absence of alpha before the script reports success (see verifyPNG).

import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

// MARK: - Brand constants

// keeper-green field. #0F6E5C — keeper's brand/primary green (the same green used for the
// app's `--primary` token in src/index.css). This native generator cannot import the CSS
// theme token, so the value is mirrored here; update both if the brand color changes.
let brandR: CGFloat = 0x0F / 255.0
let brandG: CGFloat = 0x6E / 255.0
let brandB: CGFloat = 0x5C / 255.0

// MARK: - Output map (filename -> pixel size). Must match AppIcon.appiconset/Contents.json.

let outputs: [(name: String, px: Int)] = [
    ("AppIcon-20x20@1x.png", 20),
    ("AppIcon-20x20@2x.png", 40),
    ("AppIcon-20x20@2x-1.png", 40),
    ("AppIcon-20x20@3x.png", 60),
    ("AppIcon-29x29@1x.png", 29),
    ("AppIcon-29x29@2x.png", 58),
    ("AppIcon-29x29@2x-1.png", 58),
    ("AppIcon-29x29@3x.png", 87),
    ("AppIcon-40x40@1x.png", 40),
    ("AppIcon-40x40@2x.png", 80),
    ("AppIcon-40x40@2x-1.png", 80),
    ("AppIcon-40x40@3x.png", 120),
    ("AppIcon-60x60@2x.png", 120),
    ("AppIcon-60x60@3x.png", 180),
    ("AppIcon-76x76@1x.png", 76),
    ("AppIcon-76x76@2x.png", 152),
    ("AppIcon-83.5x83.5@2x.png", 167),
    ("AppIcon-512@2x.png", 1024),
]

// MARK: - Paths

// Resolve the asset catalog relative to this script so it works from any CWD.
let scriptURL = URL(fileURLWithPath: CommandLine.arguments[0]).resolvingSymlinksInPath()
let repoRoot = scriptURL.deletingLastPathComponent().deletingLastPathComponent()
let outDir = repoRoot
    .appendingPathComponent("src-tauri/crates/keeper/gen/apple")
    .appendingPathComponent("Assets.xcassets/AppIcon.appiconset")

// Fail loudly if the resolved catalog directory does not exist, rather than silently
// writing a stray AppIcon.appiconset at the wrong path (e.g. if this script or the Tauri
// gen/apple layout ever moves).
guard FileManager.default.fileExists(atPath: outDir.path) else {
    FileHandle.standardError.write(
        "error: asset catalog not found at \(outDir.path)\n".data(using: .utf8)!)
    exit(1)
}

// MARK: - Drawing

/// Draw the full-bleed icon into an opaque RGB context of `size` x `size` pixels.
func drawIcon(size: Int) -> CGImage? {
    let dim = CGFloat(size)
    // Tag output as sRGB (not DeviceRGB) so the brand color is a color-managed, exact
    // #0F6E5C and stays consistent with the sRGB LaunchBackground colorset/storyboard.
    let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()

    // No-alpha bitmap: kCGImageAlphaNoneSkipLast produces an opaque RGB image, so the
    // encoded PNG carries no alpha channel.
    guard let ctx = CGContext(
        data: nil,
        width: size,
        height: size,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: colorSpace,
        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
    ) else {
        return nil
    }

    ctx.setAllowsAntialiasing(true)
    ctx.setShouldAntialias(true)

    // Full-bleed keeper-green background (opaque, square, no rounded corners — iOS masks).
    ctx.setFillColor(red: brandR, green: brandG, blue: brandB, alpha: 1.0)
    ctx.fill(CGRect(x: 0, y: 0, width: dim, height: dim))

    // Centered white speech-bubble "keep" mark with ~13% padding on each side.
    // Body: rounded rect occupying the padded box minus room for the tail at the bottom.
    let pad = dim * 0.13
    let box = CGRect(x: pad, y: pad, width: dim - 2 * pad, height: dim - 2 * pad)

    // Bubble body sits in the upper ~78% of the box; the tail hangs below its lower-left.
    let bodyH = box.height * 0.78
    let body = CGRect(x: box.minX, y: box.maxY - bodyH, width: box.width, height: bodyH)
    let corner = min(body.width, body.height) * 0.28

    let path = CGMutablePath()
    path.addRoundedRect(in: body, cornerWidth: corner, cornerHeight: corner)

    // Triangular tail pointing down-left from the bubble's lower-left region.
    // Coordinates are in CG's bottom-left origin space.
    let tailW = box.width * 0.20
    let tailTopY = body.minY + corner * 0.35
    let tailAnchorX = body.minX + body.width * 0.24
    let tailTipX = body.minX + body.width * 0.10
    let tailTipY = box.minY

    let tail = CGMutablePath()
    tail.move(to: CGPoint(x: tailAnchorX, y: tailTopY))
    tail.addLine(to: CGPoint(x: tailAnchorX + tailW, y: tailTopY))
    tail.addLine(to: CGPoint(x: tailTipX, y: tailTipY))
    tail.closeSubpath()

    ctx.setFillColor(red: 1, green: 1, blue: 1, alpha: 1)
    ctx.addPath(path)
    ctx.addPath(tail)
    ctx.fillPath()

    return ctx.makeImage()
}

/// Encode a CGImage to a PNG file. Strips alpha by re-hosting in an opaque RGB context
/// upstream; here we just write the bytes.
func writePNG(_ image: CGImage, to url: URL) -> Bool {
    guard let dest = CGImageDestinationCreateWithURL(
        url as CFURL, UTType.png.identifier as CFString, 1, nil
    ) else {
        return false
    }
    CGImageDestinationAddImage(dest, image, nil)
    return CGImageDestinationFinalize(dest)
}

/// Re-read a just-written PNG and confirm it is exactly `expected`x`expected` px and carries
/// no alpha channel. This is a tripwire for the App Store "app icons must be opaque" rule:
/// if a future CoreGraphics/ImageIO change ever reintroduced alpha or a wrong size, the
/// script fails loudly instead of silently committing an App-Store-rejectable icon.
func verifyPNG(at url: URL, expected: Int) -> Bool {
    guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
          let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any]
    else {
        return false
    }
    let w = props[kCGImagePropertyPixelWidth] as? Int ?? -1
    let h = props[kCGImagePropertyPixelHeight] as? Int ?? -1
    // ImageIO omits kCGImagePropertyHasAlpha for opaque PNGs, so absence == no alpha.
    let hasAlpha = props[kCGImagePropertyHasAlpha] as? Bool ?? false
    return w == expected && h == expected && !hasAlpha
}

// MARK: - Main

var failures = 0
for out in outputs {
    guard let image = drawIcon(size: out.px) else {
        FileHandle.standardError.write("failed to draw \(out.name)\n".data(using: .utf8)!)
        failures += 1
        continue
    }
    let url = outDir.appendingPathComponent(out.name)
    guard writePNG(image, to: url) else {
        FileHandle.standardError.write("failed to write \(out.name)\n".data(using: .utf8)!)
        failures += 1
        continue
    }
    guard verifyPNG(at: url, expected: out.px) else {
        FileHandle.standardError.write(
            "verification failed for \(out.name) (wrong size or has alpha)\n".data(using: .utf8)!)
        failures += 1
        continue
    }
    print("wrote \(out.name) (\(out.px)x\(out.px))")
}

if failures > 0 {
    FileHandle.standardError.write("\(failures) icon(s) failed\n".data(using: .utf8)!)
    exit(1)
}
print("generated \(outputs.count) icons into \(outDir.path)")
