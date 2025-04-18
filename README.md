# Start

* ffmpeg -i "Wuthering Waves Story Cinematics  The Ending She Desired.mp4" -r 16 -s 240x135 -f image2 ./target/images/%d.jpeg

* ffmpeg -i "Wuthering Waves Story Cinematics  The Ending She Desired.mp4" -i "videoplayback.m4a" -c:v copy -c:a aac -shortest output.mp4