#!/usr/bin/env python3
"""
Camelyon16 TIF Viewer using OpenSlide
Navigate with arrow keys, zoom with +/-
"""

import argparse
import tkinter as tk
from PIL import Image, ImageTk

try:
    import openslide
except ImportError:
    print("Please install openslide-python: pip install openslide-python")
    print("Also ensure libopenslide is installed on your system.")
    exit(1)


class SlideViewer:
    def __init__(self, slide_path, x_offset=0, y_offset=0, zoom_level=0):
        self.slide = openslide.OpenSlide(slide_path)
        self.x_offset = x_offset
        self.y_offset = y_offset
        self.zoom_level = zoom_level
        self.max_zoom = self.slide.level_count - 1
        self.initial_size = 1024
        self.view_width = self.initial_size
        self.view_height = self.initial_size
        self.nudge_amount = 256  # pixels to move per arrow key press
        self.display_scale = 1.0  # Additional scaling when at max native zoom
        self.max_display_scale = 10.0  # Maximum display scale (10x)

        # Get slide dimensions
        self.slide_width, self.slide_height = self.slide.dimensions
        print(f"Slide dimensions: {self.slide_width} x {self.slide_height}")
        print(f"Available zoom levels: {self.slide.level_count}")
        print(f"Level dimensions: {self.slide.level_dimensions}")
        print(f"Level downsamples: {self.slide.level_downsamples}")

        # Create window
        self.root = tk.Tk()
        self.root.title(f"Slide Viewer - {slide_path}")
        self.root.geometry(f"{self.initial_size}x{self.initial_size}")

        # Create canvas for image display
        self.canvas = tk.Canvas(self.root)
        self.canvas.pack(fill=tk.BOTH, expand=True)

        # Status label
        self.status_var = tk.StringVar()
        self.status_label = tk.Label(
            self.root, textvariable=self.status_var, anchor='w')
        self.status_label.pack(fill=tk.X)

        # Bind keys
        self.root.bind('<Left>', lambda e: self.nudge(-1, 0))
        self.root.bind('<Right>', lambda e: self.nudge(1, 0))
        self.root.bind('<Up>', lambda e: self.nudge(0, -1))
        self.root.bind('<Down>', lambda e: self.nudge(0, 1))
        self.root.bind('<a>', lambda e: self.nudge(-1, 0))
        self.root.bind('<d>', lambda e: self.nudge(1, 0))
        self.root.bind('<w>', lambda e: self.nudge(0, -1))
        self.root.bind('<s>', lambda e: self.nudge(0, 1))
        self.root.bind('<plus>', lambda e: self.change_zoom(-1))
        # = key (shift+= is +)
        self.root.bind('<equal>', lambda e: self.change_zoom(-1))
        self.root.bind('<minus>', lambda e: self.change_zoom(1))
        self.root.bind('<KP_Add>', lambda e: self.change_zoom(-1))  # numpad +
        self.root.bind('<KP_Subtract>',
                       lambda e: self.change_zoom(1))  # numpad -
        self.root.bind('<Escape>', lambda e: self.root.quit())

        # Mouse wheel zoom (Linux uses Button-4/5, Windows/Mac use MouseWheel)
        self.root.bind('<Button-4>', lambda e: self.change_zoom(-1))  # scroll up = zoom in
        self.root.bind('<Button-5>', lambda e: self.change_zoom(1))   # scroll down = zoom out
        self.root.bind('<MouseWheel>', self.on_mousewheel)  # Windows/Mac

        # Handle window resize
        self.canvas.bind('<Configure>', self.on_resize)

        # Initial render
        self.update_view()

    def on_mousewheel(self, event):
        """Handle mouse wheel events (Windows/Mac)."""
        if event.delta > 0:
            self.change_zoom(-1)  # scroll up = zoom in
        else:
            self.change_zoom(1)   # scroll down = zoom out

    def on_resize(self, event):
        """Handle window resize events."""
        if event.width != self.view_width or event.height != self.view_height:
            self.view_width = event.width
            self.view_height = event.height
            self.update_view()

    def nudge(self, dx, dy):
        """Move the view by nudge_amount in the given direction."""
        # Scale nudge amount by current zoom level and display scale
        downsample = self.slide.level_downsamples[self.zoom_level]
        scaled_nudge = int(self.nudge_amount * downsample / self.display_scale)

        self.x_offset += dx * scaled_nudge
        self.y_offset += dy * scaled_nudge

        # Clamp to valid range
        self.x_offset = max(0, min(self.x_offset, self.slide_width - 1))
        self.y_offset = max(0, min(self.y_offset, self.slide_height - 1))

        self.update_view()

    def change_zoom(self, delta):
        """Change zoom level (higher level = more zoomed out)."""
        if delta < 0:  # Zooming in
            if self.zoom_level > 0:
                # Still have native zoom levels to use
                self.zoom_level -= 1
                self.display_scale = 1.0
            elif self.display_scale < self.max_display_scale:
                # At max native zoom, increase display scale
                self.display_scale = min(self.display_scale * 1.5, self.max_display_scale)
            else:
                return  # Already at max zoom
        else:  # Zooming out (delta > 0)
            if self.display_scale > 1.0:
                # First reduce display scale
                self.display_scale = max(self.display_scale / 1.5, 1.0)
                if self.display_scale < 1.01:  # Close enough to 1.0
                    self.display_scale = 1.0
            elif self.zoom_level < self.max_zoom:
                # Then use native zoom levels
                self.zoom_level += 1
            else:
                return  # Already at min zoom
        self.update_view()

    def update_view(self):
        """Read region from slide and update display."""
        try:
            # Get downsample factor for current level
            downsample = self.slide.level_downsamples[self.zoom_level]

            # When display_scale > 1, we read a smaller region and scale it up
            read_width = int(self.view_width / self.display_scale)
            read_height = int(self.view_height / self.display_scale)

            # Calculate the region size at level 0 coordinates
            region_width_l0 = int(read_width * downsample)
            region_height_l0 = int(read_height * downsample)

            # Clamp offsets to ensure we don't read outside the image
            x = max(0, min(self.x_offset, self.slide_width - region_width_l0))
            y = max(0, min(self.y_offset, self.slide_height - region_height_l0))

            # Read region from slide
            # read_region takes (location, level, size)
            # location is in level 0 coordinates
            # size is in the requested level's coordinates
            region = self.slide.read_region(
                (x, y),
                self.zoom_level,
                (read_width, read_height)
            )

            # Convert RGBA to RGB (openslide returns RGBA)
            region = region.convert('RGB')

            # Scale up if display_scale > 1
            if self.display_scale > 1.0:
                region = region.resize(
                    (self.view_width, self.view_height),
                    Image.NEAREST  # Use nearest neighbor for sharp pixels
                )

            # Convert to PhotoImage for tkinter
            self.photo = ImageTk.PhotoImage(region)

            # Update canvas
            self.canvas.delete('all')
            self.canvas.create_image(0, 0, anchor=tk.NW, image=self.photo)

            # Update status
            effective_zoom = downsample / self.display_scale
            scale_info = f" | Display scale: {self.display_scale:.1f}x" if self.display_scale > 1.0 else ""
            self.status_var.set(
                f"Position: ({x}, {y}) | "
                f"Zoom level: {self.zoom_level}/{self.max_zoom} | "
                f"Effective: {effective_zoom:.2f}x{scale_info} | "
                f"[Arrows: pan, +/-: zoom, Q/Esc: quit]"
            )

        except Exception as e:
            print(f"Error reading region: {e}")
            self.status_var.set(f"Error: {e}")

    def run(self):
        """Start the main event loop."""
        self.root.mainloop()
        self.slide.close()


def main():
    parser = argparse.ArgumentParser(
        description='View Camelyon16 TIF slides using OpenSlide',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Controls:
  Arrow keys    Pan the view
  +/=           Zoom in (lower level number)
  -             Zoom out (higher level number)
  Q/Escape      Quit

Examples:
  %(prog)s slide.tif
  %(prog)s slide.tif -x 10000 -y 20000 -z 2
        """
    )
    parser.add_argument('slide_path', help='Path to the TIF/SVS slide file')
    parser.add_argument('-x', '--x-offset', type=int, default=0,
                        help='Initial X offset in level 0 coordinates (default: 0)')
    parser.add_argument('-y', '--y-offset', type=int, default=0,
                        help='Initial Y offset in level 0 coordinates (default: 0)')
    parser.add_argument('-z', '--zoom', type=int, default=0,
                        help='Initial zoom level (0=highest resolution, default: 0)')

    args = parser.parse_args()

    try:
        viewer = SlideViewer(
            args.slide_path,
            x_offset=args.x_offset,
            y_offset=args.y_offset,
            zoom_level=args.zoom
        )
        viewer.run()
    except openslide.OpenSlideError as e:
        print(f"Error opening slide: {e}")
        exit(1)
    except FileNotFoundError:
        print(f"File not found: {args.slide_path}")
        exit(1)


if __name__ == '__main__':
    main()
