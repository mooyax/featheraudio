// reference: decode an OGG with stb_vorbis frame by frame, write "ch rate" then interleaved floats
#define STB_VORBIS_NO_PUSHDATA_API
#include "stb_vorbis.c"
int main(int argc, char **argv) {
  FILE *f = fopen(argv[1], "rb"); if (!f) return 2;
  fseek(f, 0, SEEK_END); long n = ftell(f); fseek(f, 0, SEEK_SET);
  unsigned char *d = malloc(n ? n : 1); if (fread(d, 1, n, f) != (size_t)n) return 2; fclose(f);
  int err; stb_vorbis *v = stb_vorbis_open_memory(d, (int)n, &err, NULL);
  FILE *o = fopen(argv[2], "wb");
  if (!v) { fprintf(o, "ERR\n"); fclose(o); return 0; }
  fprintf(o, "%d %u\n", v->channels, v->sample_rate);
  int rounds = argc > 3 ? atoi(argv[3]) : 1;
  for (int r = 0; r < rounds; r++) {
    if (r) stb_vorbis_seek_start(v);
    for (;;) {
      int len, left, right;
      // what stb_vorbis_get_frame_float does, but telling the end apart from empty frames
      if (!vorbis_decode_packet(v, &len, &left, &right)) break;
      len = vorbis_finish_frame(v, len, left, right);
      for (int i = 0; i < len; i++) for (int c = 0; c < v->channels; c++) fwrite(&v->channel_buffers[c][left + i], 4, 1, o);
    }
  }
  fclose(o); stb_vorbis_close(v); free(d); return 0;
}
