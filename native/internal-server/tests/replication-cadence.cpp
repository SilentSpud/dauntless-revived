#ifdef _WIN32
#include <Windows.h>
#endif
#include "../ReplicationCadence.h"
#include <cassert>
#include <iostream>
#include <limits>
int main() {
    ReplicationCadence cadence;int actor=0, moving=0;
    int slowUpdates=0, movementUpdates=0;
    for (int tick=0;tick<300;tick++) {
        double now=tick/30.0;
        slowUpdates+=cadence.Due(&actor,1,now,1,false);
        movementUpdates+=cadence.Due(&moving,2,now,1,true);
        cadence.Prune(now);
    }
    assert(slowUpdates==100);assert(movementUpdates==300);
    assert(cadence.Due(&actor,3,9.98,1,false)); // reused address, different object
    assert(cadence.Due(&actor,3,9.99,1,true)); // urgent update bypasses cadence
    assert(cadence.Due(&actor,3,9.99,100,false));
    assert(cadence.Due(&actor,3,9.99,0,false));
    assert(cadence.Due(&actor,3,9.99,std::numeric_limits<float>::quiet_NaN(),false));
    cadence.Prune(12);assert(cadence.Due(&actor,3,12,1,false));
    cadence.Clear();assert(cadence.Due(&actor,3,12,1,false));
    std::cout<<"replication cadence: 3x fewer low-rate updates, full movement, reuse/urgent/prune checks passed\n";
}
