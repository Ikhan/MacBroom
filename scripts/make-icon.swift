// Renders the 1024x1024 app icon PNG: gradient squircle with a broom emoji.
import AppKit

let size: CGFloat = 1024
let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "icon.png"
let image = NSImage(size: NSSize(width: size, height: size))
image.lockFocus()
let rect = NSRect(x: 80, y: 80, width: size - 160, height: size - 160)
let path = NSBezierPath(roundedRect: rect, xRadius: 190, yRadius: 190)
NSGradient(colors: [
    NSColor(calibratedRed: 0.55, green: 0.20, blue: 0.85, alpha: 1),
    NSColor(calibratedRed: 0.95, green: 0.30, blue: 0.60, alpha: 1),
])!.draw(in: path, angle: -60)
let emoji = NSAttributedString(string: "🧹", attributes: [.font: NSFont.systemFont(ofSize: 560)])
let e = emoji.size()
emoji.draw(at: NSPoint(x: (size - e.width) / 2, y: (size - e.height) / 2 - 10))
image.unlockFocus()
guard let tiff = image.tiffRepresentation,
      let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) else {
    fatalError("could not render icon")
}
try! png.write(to: URL(fileURLWithPath: out))
