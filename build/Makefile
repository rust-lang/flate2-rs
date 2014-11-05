ifneq ($(findstring windows,$(TARGET)),)
	CC := gcc
endif

ifneq ($(findstring i686,$(TARGET)),)
	CC := $(CC) -m32
else ifneq ($(findstring x86_64,$(TARGET)),)
	CC := $(CC) -m64 -fPIC
else
	CC := $(CC) -fPIC
endif

all:
	$(CC) $(CFLAGS) -O2 miniz.c -c -o $(OUT_DIR)/miniz.o
	$(AR) crus $(OUT_DIR)/libminiz.a $(OUT_DIR)/miniz.o
