#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Bridge + cantilever + 40deg overhang test part (extruded XZ profile), watertight binary STL."""
import struct, sys
P = [(0,0),(10,0),(10,15),(40,15),(40,0),(50,0),(50,15),(65,15),(65,20),(-12,20),(0,10)]
DEPTH = 20.0
def area(p): return sum(p[i][0]*p[(i+1)%len(p)][1]-p[(i+1)%len(p)][0]*p[i][1] for i in range(len(p)))/2
if area(P) < 0: P = P[::-1]
def inside(a,b,c,p):
    def s(p1,p2,p3): return (p1[0]-p3[0])*(p2[1]-p3[1])-(p2[0]-p3[0])*(p1[1]-p3[1])
    d1,d2,d3=s(p,a,b),s(p,b,c),s(p,c,a)
    return not ((d1<0 or d2<0 or d3<0) and (d1>0 or d2>0 or d3>0))
def ear(poly):
    idx=list(range(len(poly))); tris=[]
    while len(idx)>3:
        for k in range(len(idx)):
            i,j,l=idx[k-1],idx[k],idx[(k+1)%len(idx)]
            a,b,c=poly[i],poly[j],poly[l]
            if (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])<=0: continue
            if any(inside(a,b,c,poly[m]) for m in idx if m not in (i,j,l)): continue
            tris.append((i,j,l)); idx.pop(k); break
    tris.append(tuple(idx)); return tris
tris=[]
def v(x,z,y): return (x+20.0, y+100.0, z)  # shift onto the bed area (x>=8), y centred-ish
for (i,j,l) in ear(P):
    a,b,c=P[i],P[j],P[l]
    # profile is in XZ; extrude along +Y. front face y=0 normal -Y, back y=DEPTH normal +Y
    tris.append((v(a[0],a[1],0),v(c[0],c[1],0),v(b[0],b[1],0)))
    tris.append((v(a[0],a[1],DEPTH),v(b[0],b[1],DEPTH),v(c[0],c[1],DEPTH)))
for k in range(len(P)):
    a,b=P[k],P[(k+1)%len(P)]
    tris.append((v(a[0],a[1],0),v(b[0],b[1],0),v(b[0],b[1],DEPTH)))
    tris.append((v(a[0],a[1],0),v(b[0],b[1],DEPTH),v(a[0],a[1],DEPTH)))
tris=[(t[0],t[2],t[1]) for t in tris]  # (x,z,y) mapping flips handedness
def n(t):
    (x1,y1,z1),(x2,y2,z2),(x3,y3,z3)=t
    ux,uy,uz=x2-x1,y2-y1,z2-z1; vx,vy,vz=x3-x1,y3-y1,z3-z1
    nx,ny,nz=uy*vz-uz*vy,uz*vx-ux*vz,ux*vy-uy*vx; L=(nx*nx+ny*ny+nz*nz)**.5 or 1
    return nx/L,ny/L,nz/L
out=sys.argv[1] if len(sys.argv)>1 else "bridge-overhang.stl"
with open(out,"wb") as f:
    f.write(b"Shale bridge/cantilever/overhang test part".ljust(80,b" ")); f.write(struct.pack("<I",len(tris)))
    for t in tris: f.write(struct.pack("<12fH",*n(t),*t[0],*t[1],*t[2],0))
# sanity: signed volume
vol=sum((t[0][0]*(t[1][1]*t[2][2]-t[1][2]*t[2][1])-t[0][1]*(t[1][0]*t[2][2]-t[1][2]*t[2][0])+t[0][2]*(t[1][0]*t[2][1]-t[1][1]*t[2][0]))/6 for t in tris)
print(out,len(tris),"tris volume",round(vol,2),"expected",round(area(P)*DEPTH,2))
