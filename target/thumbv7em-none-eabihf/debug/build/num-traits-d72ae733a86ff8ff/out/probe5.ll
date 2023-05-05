; ModuleID = 'probe5.055fc8c7-cgu.0'
source_filename = "probe5.055fc8c7-cgu.0"
target datalayout = "e-m:e-p:32:32-Fi8-i64:64-v128:64:128-a:0:32-n32-S64"
target triple = "thumbv7em-none-unknown-eabihf"

; probe5::probe
; Function Attrs: nounwind
define dso_local void @_ZN6probe55probe17haaec5898f4954077E() unnamed_addr #0 {
start:
  %0 = alloca i32, align 4
  store i32 1, ptr %0, align 4
  %1 = load i32, ptr %0, align 4, !noundef !0
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind readnone speculatable willreturn
declare i32 @llvm.cttz.i32(i32, i1 immarg) #1

attributes #0 = { nounwind "frame-pointer"="all" "target-cpu"="generic" "target-features"="+vfp4,-d32,-fp64" }
attributes #1 = { nocallback nofree nosync nounwind readnone speculatable willreturn }

!0 = !{}
