// Stand-in for decklink-helper --play-shm, reading through ring.h itself.
// argv: name width height frames. Each frame is one repeated byte.
#include "ring.h"
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <thread>
#include <vector>
int main(int argc, char** argv) {
  if (argc != 5) return 2;
  const uint32_t w = std::atoi(argv[2]), h = std::atoi(argv[3]);
  const uint64_t quiet_limit = 3000;
  const int fd = shm_open(argv[1], O_RDONLY, 0);
  if (fd < 0) { puts("{\"error\":\"Frame ring not found\"}"); return 3; }
  struct stat info;
  if (fstat(fd, &info) || info.st_size < (off_t)kairo_ring::HEADER_BYTES) { puts("{\"error\":\"too small\"}"); return 3; }
  void* base = mmap(nullptr, info.st_size, PROT_READ, MAP_SHARED, fd, 0);
  auto* header = static_cast<const kairo_ring::Header*>(base);
  if (!kairo_ring::compatible(header, w, h)) { puts("{\"error\":\"raster mismatch\"}"); return 4; }
  const size_t bytes = kairo_ring::frameBytes(w, h);
  std::vector<uint8_t> frame(bytes);
  uint64_t last = 0, read = 0, torn = 0, quiet = 0;
  while (quiet < quiet_limit) {
    if (kairo_ring::readNewest(header, base, last, frame.data(), bytes)) {
      ++read; quiet = 0;
      for (uint8_t b : frame) if (b != frame[0]) { ++torn; break; }
    } else { ++quiet; std::this_thread::sleep_for(std::chrono::milliseconds(1)); }
  }
  printf("{\"read\":%llu,\"torn\":%llu,\"published\":%llu}\n", (unsigned long long)read, (unsigned long long)torn,
         (unsigned long long)header->published.load());
  return 0;
}
