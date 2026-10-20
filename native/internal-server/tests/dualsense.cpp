#include "../DualSense.h"
#include <cassert>
#include <iostream>
int main() {
 DualSense::Fields f{};
 assert(!DualSense::ParseReport(nullptr,0,f));
 uint8_t report[78]{};
 assert(!DualSense::ParseReport(report,0,f));
 report[0]=1;
 assert(!DualSense::ParseReport(report,9,f));
 report[1]=128;report[2]=0;report[3]=255;report[4]=128;
 report[5]=21;report[6]=42;report[8]=0x28;report[9]=0x20;report[10]=2;
 assert(DualSense::ParseReport(report,64,f));
 auto p=DualSense::ToXInput(f);
 assert(p.bLeftTrigger==21 && p.bRightTrigger==42);
 assert(p.sThumbLX==0 && p.sThumbLY==32767 && p.sThumbRX==32766);
 assert(p.wButtons==(XINPUT_GAMEPAD_A|XINPUT_GAMEPAD_START|XINPUT_GAMEPAD_BACK));
 report[0]=0x31;
 assert(!DualSense::ParseReport(report,11,f));
 assert(DualSense::ParseReport(report,78,f));
 report[0]=1;report[5]=0x18;report[6]=1;report[7]=0;report[8]=55;report[9]=66;
 assert(DualSense::ParseReport(report,10,f));
 p=DualSense::ToXInput(f);
 assert(p.bLeftTrigger==55 && p.bRightTrigger==66);
 assert(p.wButtons==(XINPUT_GAMEPAD_X|XINPUT_GAMEPAD_LEFT_SHOULDER));
 DualSense::RealPads=1;
 assert(!DualSense::IsOurSlot(0) && DualSense::IsOurSlot(1));
 DualSense::RealPads=15;
 assert(!DualSense::IsOurSlot(0) && !DualSense::IsOurSlot(3));
 const uint8_t crc[]={'1','2','3','4','5','6','7','8','9'};
 assert(~DualSense::Crc32(crc,sizeof crc)==0xcbf43926);
 std::cout<<"DualSense report, mapping, CRC and slot tests passed\n";
}
