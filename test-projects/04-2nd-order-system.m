% Reference for 04-2nd-order-system.json
%
%   step -(gain 1)-> [sum + -] --e--> C(z) --u--> P(z) --y--> scope
%                        ^                                     |
%                        +------------------- y ---------------+
%
% A discrete controller C (tf-17) and discrete plant P (tf-9) in unity feedback,
% driven by a unit step at t = 1 s. Both blocks are already discrete at
% Ts = 0.05 (z domain, highest power first), so no c2d is needed -- the
% difference-equation coefficients are read straight off the model.
%
% Both blocks are strictly proper (leading numerator coefficient 0), so each
% output depends on past inputs/outputs only; the loop needs no algebraic solve.
% This mirrors exactly what the backend and the firmware execute per sample.
%
% Run in MATLAB to (re)generate 04-2nd-order-system.ref.csv.

Ts = 0.05; end_time = 25;
t = (0:Ts:end_time)';
N = numel(t);

% Plant P (tf-9): y[k] = 0.0178 u[k-1] + 0.0177 u[k-2] + 1.9696 y[k-1] - 0.9739 y[k-2]
% Controller C (tf-17): u[k] = 0.0015 e[k-1] - 6.5765e-4 e[k-2] + 1.4601 u[k-1] - 0.4709 u[k-2]
Pb = [0 0.0178 0.0177];        Pa = [1 -1.9696 0.9739];
Cb = [0 0.0015 -6.5765e-04];   Ca = [1 -1.4601 0.4709];

r = double(t >= 1.0);          % step-11 (gain-15 has gain 1, so g = r)

tf9  = zeros(N,1);             % plant output y
tf17 = zeros(N,1);             % controller output u
sum16 = zeros(N,1);            % error e
U1 = 0; U2 = 0;               % u[k-1], u[k-2]
Y1 = 0; Y2 = 0;               % y[k-1], y[k-2]
E1 = 0; E2 = 0;               % e[k-1], e[k-2]
for k = 1:N
    y = Pb(2)*U1 + Pb(3)*U2 - Pa(2)*Y1 - Pa(3)*Y2;   % plant from past
    e = r(k) - y;                                     % sum "+ -"
    u = Cb(2)*E1 + Cb(3)*E2 - Ca(2)*U1 - Ca(3)*U2;    % controller from past

    tf9(k) = y; sum16(k) = e; tf17(k) = u;
    U2 = U1; U1 = u;  Y2 = Y1; Y1 = y;  E2 = E1; E1 = e;
end

header = {'t','step-11','gain-15','sum-16','transferFunction-17', ...
          'transferFunction-9','scope-10'};
data = [t, r, r, sum16, tf17, tf9, tf9];

here = fileparts(mfilename('fullpath'));
if isempty(here); here = pwd; end
out = fullfile(here, '04-2nd-order-system.ref.csv');
fid = fopen(out, 'w');
fprintf(fid, '%s\n', strjoin(header, ','));
fmt = [repmat('%.12g,', 1, size(data,2)-1) '%.12g\n'];
fprintf(fid, fmt, data.');
fclose(fid);
fprintf('wrote %s (%d samples)\n', out, size(data,1));
