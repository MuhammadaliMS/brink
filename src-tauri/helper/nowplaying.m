// Brink's Now Playing reader and remote. Since macOS 15.4 only Apple-signed processes may read what's
// playing, so Brink runs this inside /usr/bin/perl (an Apple platform binary), which loads it
// with DynaLoader and calls brink_np_loop. Each "get" line on stdin answers one JSON line:
// {"elapsed": seconds now, "duration": seconds, "rate": 0|1}; {"rate": 0|1} for something with no
// length (a live stream); {} when no app is in Now Playing; {"unknown": true} when it didn't answer.
// "set <seconds>" moves the playhead and "cmd <n>" sends a MediaRemote command (2 play/pause,
// 4 next, 5 previous); neither answers. Commands go out from here too, not from Brink: what's
// playing honours them from an Apple process. It exits when Brink closes stdin (Brink quit).
#import <Foundation/Foundation.h>
#include <dlfcn.h>
#include <stdlib.h>
#include <string.h>

typedef void (*GetInfo)(dispatch_queue_t, void (^)(CFDictionaryRef));
typedef void (*SetElapsed)(double);
typedef bool (*SendCommand)(unsigned, CFDictionaryRef);

static NSString *answer(GetInfo info) {
  dispatch_semaphore_t done = dispatch_semaphore_create(0);
  // No reply in time: say so, rather than claim nothing is playing.
  __block NSDictionary *out = @{@"unknown" : @YES};
  info(dispatch_get_global_queue(QOS_CLASS_USER_INTERACTIVE, 0), ^(CFDictionaryRef d) {
    NSDictionary *n = (__bridge NSDictionary *)d;
    NSNumber *elapsed = n[@"kMRMediaRemoteNowPlayingInfoElapsedTime"];
    NSNumber *duration = n[@"kMRMediaRemoteNowPlayingInfoDuration"];
    NSNumber *rate = n[@"kMRMediaRemoteNowPlayingInfoPlaybackRate"];
    NSDate *stamp = n[@"kMRMediaRemoteNowPlayingInfoTimestamp"];
    if (n.count == 0) {
      out = @{};  // nothing in Now Playing at all
    } else if (duration.doubleValue <= 0) {
      out = @{@"rate" : rate ?: @0};  // something with no length (a live stream)
    } else {
      double e = elapsed.doubleValue;
      // The elapsed time is as of the timestamp: move it on to now while playing.
      if (rate.doubleValue > 0 && [stamp isKindOfClass:NSDate.class]) e += rate.doubleValue * -stamp.timeIntervalSinceNow;
      out = @{@"elapsed" : @(e), @"duration" : duration, @"rate" : rate ?: @0};
    }
    dispatch_semaphore_signal(done);
  });
  dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, (int64_t)(1.5 * NSEC_PER_SEC)));
  NSData *j = [NSJSONSerialization dataWithJSONObject:out options:0 error:nil];
  return [[NSString alloc] initWithData:j encoding:NSUTF8StringEncoding];
}

void brink_np_loop(void) {
  void *h = dlopen("/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote", RTLD_NOW);
  GetInfo info = h ? (GetInfo)dlsym(h, "MRMediaRemoteGetNowPlayingInfo") : NULL;
  SetElapsed set = h ? (SetElapsed)dlsym(h, "MRMediaRemoteSetElapsedTime") : NULL;
  SendCommand send = h ? (SendCommand)dlsym(h, "MRMediaRemoteSendCommand") : NULL;
  char line[64];
  while (fgets(line, sizeof line, stdin)) {
    @autoreleasepool {
      if (strncmp(line, "set ", 4) == 0) {
        if (set) set(atof(line + 4));
        continue;
      }
      if (strncmp(line, "cmd ", 4) == 0) {
        if (send) send((unsigned)atoi(line + 4), NULL);
        continue;
      }
      NSString *a = (info && strncmp(line, "get", 3) == 0) ? answer(info) : @"{}";
      fprintf(stdout, "%s\n", a.UTF8String);
      fflush(stdout);
    }
  }
}
