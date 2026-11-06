# Contoh paket .bzpkg: dijalankan dengan "halo-paket [arg...]" setelah dipasang.
echo "Halo dari paket halo-paket v1.2.0!"
if [ $# -gt 0 ]; then
    for a in $@; do
        echo "  argumen: $a"
    done
else
    echo "  (tanpa argumen)"
fi
