`video.mp4` is a synthetic two-second black H.264 video (128×72, 10 FPS, no audio).
It is used by file-only multi-track mux tests and contains no user recording.

Regenerate with FFmpeg: `ffmpeg -f lavfi -i color=c=black:s=128x72:r=10:d=2 -an -c:v libx264 -bf 0 -pix_fmt yuv420p -y video.mp4`.
FFmpeg is a development fixture-generation tool only; it is not needed at runtime
or during the Rust test. The audio test writes PCM files and never plays them.
